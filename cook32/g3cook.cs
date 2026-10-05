// g3cook: cooks triangle meshes with Gothic 3's own PhysX cooking library (NxCooking.dll, 32-bit), so the game
// gets collision streams exactly as its PhysX reads them. Part of Gothic 3 ImpExp; copyright (C) 2026 UniLinkIF,
// GPL-3.0-or-later with additional terms (see NOTICE). The PhysX library is the player's own, loaded from the game
// folder; it is not shipped.
//
//   g3cook.exe <game folder> <in.bin> <out.bin>
//   in : u32 meshes · per mesh: u32 vertices · u32 triangles · f32 xyz[vertices] (metres) · u32 abc[triangles]
//        · u16 material[triangles] (the game's own streams carry 1 on every triangle)
//   out: u32 meshes · per mesh: u32 bytes · the cooked stream ("NXS\x01MESH" ...)
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;

static class G3Cook
{
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool SetDllDirectory(string path);
    [DllImport("NxCooking.dll", EntryPoint = "_NxInitCooking@8", CallingConvention = CallingConvention.StdCall)] static extern bool NxInitCooking(IntPtr allocator, IntPtr output);
    [DllImport("NxCooking.dll", EntryPoint = "_NxCookTriangleMesh@8", CallingConvention = CallingConvention.StdCall)] static extern bool NxCookTriangleMesh(IntPtr desc, IntPtr stream);
    [DllImport("NxCooking.dll", EntryPoint = "_NxCloseCooking@0", CallingConvention = CallingConvention.StdCall)] static extern void NxCloseCooking();

    // NxStream: a C++ interface, called with thiscall through its vtable.
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate IntPtr Dtor(IntPtr self, uint flags);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate byte RdByte(IntPtr self);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate ushort RdWord(IntPtr self);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate uint RdDword(IntPtr self);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate float RdFloat(IntPtr self);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate double RdDouble(IntPtr self);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate void RdBuffer(IntPtr self, IntPtr buf, uint size);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate IntPtr StByte(IntPtr self, byte b);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate IntPtr StWord(IntPtr self, ushort w);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate IntPtr StDword(IntPtr self, uint d);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate IntPtr StFloat(IntPtr self, float f);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate IntPtr StDouble(IntPtr self, double d);
    [UnmanagedFunctionPointer(CallingConvention.ThisCall)] delegate IntPtr StBuffer(IntPtr self, IntPtr buf, uint size);

    static MemoryStream sink = new MemoryStream();
    static List<Delegate> keep = new List<Delegate>();

    static IntPtr Fn(Delegate d) { keep.Add(d); return Marshal.GetFunctionPointerForDelegate(d); }

    static IntPtr MakeStream()
    {
        IntPtr[] slots = {
            Fn(new Dtor((s, f) => s)),
            Fn(new RdByte(s => 0)), Fn(new RdWord(s => 0)), Fn(new RdDword(s => 0)), Fn(new RdFloat(s => 0)), Fn(new RdDouble(s => 0)),
            Fn(new RdBuffer((s, b, n) => { })),
            Fn(new StByte((s, b) => { sink.WriteByte(b); return s; })),
            Fn(new StWord((s, w) => { sink.Write(BitConverter.GetBytes(w), 0, 2); return s; })),
            Fn(new StDword((s, d) => { sink.Write(BitConverter.GetBytes(d), 0, 4); return s; })),
            Fn(new StFloat((s, f) => { sink.Write(BitConverter.GetBytes(f), 0, 4); return s; })),
            Fn(new StDouble((s, d) => { sink.Write(BitConverter.GetBytes(d), 0, 8); return s; })),
            Fn(new StBuffer((s, b, n) => { var a = new byte[n]; Marshal.Copy(b, a, 0, (int)n); sink.Write(a, 0, (int)n); return s; })),
        };
        IntPtr vt = Marshal.AllocHGlobal(IntPtr.Size * slots.Length);
        for (int i = 0; i < slots.Length; i++) Marshal.WriteIntPtr(vt, i * IntPtr.Size, slots[i]);
        IntPtr obj = Marshal.AllocHGlobal(16);
        Marshal.WriteIntPtr(obj, vt);
        return obj;
    }

    static int Main(string[] args)
    {
        if (args.Length != 3) { Console.Error.WriteLine("g3cook <game folder> <in.bin> <out.bin>"); return 2; }
        if (IntPtr.Size != 4) { Console.Error.WriteLine("g3cook must run as a 32-bit process"); return 2; }
        if (!File.Exists(Path.Combine(args[0], "NxCooking.dll"))) { Console.Error.WriteLine("NxCooking.dll not found in " + args[0]); return 2; }
        SetDllDirectory(args[0]);
        if (!NxInitCooking(IntPtr.Zero, IntPtr.Zero)) { Console.Error.WriteLine("NxInitCooking failed"); return 1; }
        IntPtr stream = MakeStream();
        var input = new BinaryReader(File.OpenRead(args[1]));
        var output = new BinaryWriter(File.Create(args[2]));
        uint meshes = input.ReadUInt32();
        output.Write(meshes);
        for (uint m = 0; m < meshes; m++)
        {
            uint nv = input.ReadUInt32(), nt = input.ReadUInt32();
            byte[] pts = input.ReadBytes((int)nv * 12), tris = input.ReadBytes((int)nt * 12), mats = input.ReadBytes((int)nt * 2);
            IntPtr p = Marshal.AllocHGlobal(pts.Length), t = Marshal.AllocHGlobal(tris.Length), mt = Marshal.AllocHGlobal(Math.Max(2, mats.Length));
            Marshal.Copy(pts, 0, p, pts.Length); Marshal.Copy(tris, 0, t, tris.Length); Marshal.Copy(mats, 0, mt, mats.Length);
            // NxTriangleMeshDesc (PhysX 2.x, no vtable): NxSimpleTriangleMesh then the triangle-mesh fields.
            IntPtr desc = Marshal.AllocHGlobal(256);
            for (int i = 0; i < 256; i += 4) Marshal.WriteInt32(desc, i, 0);
            Marshal.WriteInt32(desc, 0, (int)nv);
            Marshal.WriteInt32(desc, 4, (int)nt);
            Marshal.WriteInt32(desc, 8, 12);
            Marshal.WriteInt32(desc, 12, 12);
            Marshal.WriteIntPtr(desc, 16, p);
            Marshal.WriteIntPtr(desc, 20, t);
            Marshal.WriteInt32(desc, 24, 0);                       // flags: 32-bit indices
            Marshal.WriteInt32(desc, 28, 2);                       // materialIndexStride: u16
            Marshal.WriteIntPtr(desc, 32, mt);                     // materialIndices
            Marshal.WriteInt32(desc, 36, 0xff);                    // heightFieldVerticalAxis: none
            Marshal.WriteInt32(desc, 40, 0);                       // heightFieldVerticalExtent
            Marshal.WriteIntPtr(desc, 44, IntPtr.Zero);            // pmap
            Marshal.WriteInt32(desc, 48, BitConverter.ToInt32(BitConverter.GetBytes(0.001f), 0));   // convexEdgeThreshold
            sink.SetLength(0);
            if (!NxCookTriangleMesh(desc, stream)) { Console.Error.WriteLine("NxCookTriangleMesh failed for mesh " + m); return 1; }
            byte[] cooked = sink.ToArray();
            output.Write((uint)cooked.Length);
            output.Write(cooked);
            Marshal.FreeHGlobal(p); Marshal.FreeHGlobal(t); Marshal.FreeHGlobal(mt); Marshal.FreeHGlobal(desc);
        }
        output.Close();
        NxCloseCooking();
        return 0;
    }
}
