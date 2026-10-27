"""Frame effects of a Gothic 3 clip (footsteps, hit sounds, …): kept on the action as the custom property
`g3_frame_effects`, a dict of Blender frame -> effect names (several on one frame joined by `|`, as a clip often has a
footstep and a sound together), shown and edited in the Gothic 3 panel. Motion export writes them into the clip (the
game keeps them per key frame)."""

import bpy
from bpy.props import StringProperty

PROP = "g3_frame_effects"
SEP = "|"


def _fps(scene):
    return scene.render.fps / scene.render.fps_base


def store(scene, action, effects):
    """`effects` = [(time s, name)] from the core."""
    start = action.frame_range[0]
    fx = {}
    for t, n in effects:
        fx.setdefault(str(int(round(start + t * _fps(scene)))), []).append(n)
    action[PROP] = {f: SEP.join(ns) for f, ns in fx.items()}


def as_times(scene, action):
    """The action's effects as [(time s, name)], or None when it has none of its own (the clip's stay)."""
    if PROP not in action:
        return None
    start = action.frame_range[0]
    return sorted(((int(f) - start) / _fps(scene), n) for f, ns in action[PROP].items() for n in _names(ns))


def _names(value):
    return [n for n in str(value).split(SEP) if n]


def _action(context):
    o = context.active_object
    return o.animation_data.action if o and o.type == "ARMATURE" and "g3_actor" in o and o.animation_data else None


def _known(self, context, edit_text):
    names = sorted({n for a in bpy.data.actions if PROP in a for ns in a[PROP].values() for n in _names(ns)})
    return [n for n in names if edit_text.lower() in n.lower()][:200]


class G3_OT_effect_add(bpy.types.Operator):
    bl_idname = "gothic3.effect_add"
    bl_label = "Додати ефект"
    bl_description = "Ефект (звук, крок) на поточному кадрі активної дії"
    bl_options = {"REGISTER", "UNDO"}

    name: StringProperty(name="Ефект", description="Назва ефекту гри, напр. eff_step_creature01_walk_earth_01", search=_known)

    @classmethod
    def poll(cls, context):
        return _action(context) is not None

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self, width=420)

    def execute(self, context):
        if not self.name:
            return {"CANCELLED"}
        a = _action(context)
        fx = dict(a.get(PROP, {}))
        f = str(context.scene.frame_current)
        fx[f] = SEP.join(_names(fx.get(f, "")) + [self.name])
        a[PROP] = fx
        return {"FINISHED"}


class G3_OT_effect_remove(bpy.types.Operator):
    bl_idname = "gothic3.effect_remove"
    bl_label = "Прибрати ефект"
    bl_options = {"REGISTER", "UNDO"}

    frame: StringProperty()
    name: StringProperty()

    def execute(self, context):
        a = _action(context)
        if a is not None and PROP in a:
            fx = dict(a[PROP])
            rest = [n for n in _names(fx.get(self.frame, "")) if n != self.name]
            if rest:
                fx[self.frame] = SEP.join(rest)
            else:
                fx.pop(self.frame, None)
            a[PROP] = fx
        return {"FINISHED"}


def draw_panel(layout, context):
    a = _action(context)
    if a is None:
        return
    box = layout.box()
    row = box.row()
    row.label(text=f"Ефекти кадрів: {a.name[:28]}", icon="SPEAKER")
    row.operator("gothic3.effect_add", text="", icon="ADD")
    fx = a.get(PROP, {})
    if not fx:
        box.label(text="Немає" if PROP in a else "Як у кліпі гри")
    for f in sorted(fx.keys(), key=int):
        for n in _names(fx[f]):
            r = box.row()
            r.label(text=f"{f}: {n}")
            op = r.operator("gothic3.effect_remove", text="", icon="X")
            op.frame, op.name = f, n


_classes = (G3_OT_effect_add, G3_OT_effect_remove)


def register():
    for c in _classes:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
