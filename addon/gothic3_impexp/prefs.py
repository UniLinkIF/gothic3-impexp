"""Add-on preferences: where Gothic 3 is, where the core and the cache are, and the modder's own texture folders."""

import bpy
from bpy.props import CollectionProperty, IntProperty, StringProperty

ADDON_ID = __package__


class G3_PG_TextureDir(bpy.types.PropertyGroup):
    path: StringProperty(name="Тека", subtype="DIR_PATH")


class G3_UL_TextureDirs(bpy.types.UIList):
    def draw_item(self, context, layout, data, item, icon, active_data, active_propname, index):
        layout.prop(item, "path", text="", emboss=False)


class G3_OT_TextureDirAdd(bpy.types.Operator):
    bl_idname = "gothic3.texture_dir_add"
    bl_label = "Додати теку"

    def execute(self, context):
        p = prefs(context)
        p.texture_dirs.add()
        p.texture_dir_index = len(p.texture_dirs) - 1
        return {"FINISHED"}


class G3_OT_TextureDirRemove(bpy.types.Operator):
    bl_idname = "gothic3.texture_dir_remove"
    bl_label = "Прибрати теку"

    def execute(self, context):
        p = prefs(context)
        if 0 <= p.texture_dir_index < len(p.texture_dirs):
            p.texture_dirs.remove(p.texture_dir_index)
            p.texture_dir_index = max(0, p.texture_dir_index - 1)
        return {"FINISHED"}


class G3ImpExpPreferences(bpy.types.AddonPreferences):
    bl_idname = ADDON_ID

    game_dir: StringProperty(
        name="Папка гри Gothic 3",
        description="Папка з Gothic3.exe; моделі, матеріали й текстури читаються з архівів гри (Data\\*.pak) напряму",
        subtype="DIR_PATH",
        default=r"C:\Program Files (x86)\Steam\steamapps\common\Gothic 3",
    )
    core_exe: StringProperty(
        name="Ядро (gothic3-core.exe)",
        description="Порожньо — ядро, що йде з аддоном. Лише gothic3-core.exe: інші програми аддон не запускає",
        subtype="FILE_PATH",
    )
    cache_dir: StringProperty(
        name="Кеш",
        description="Куди складати розпаковані текстури й моделі (типово — тимчасова папка)",
        subtype="DIR_PATH",
    )
    mods_dir: StringProperty(
        name="Папка модів",
        description="Куди «Пакет мода» кладе томи з INSTALL.bat / ROLLBACK.bat",
        subtype="DIR_PATH",
    )
    author: StringProperty(
        name="Автор модів",
        description="Підпис у файлах модів (.g3mod), які видно в лаунчері",
    )
    texture_dirs: CollectionProperty(type=G3_PG_TextureDir)
    texture_dir_index: IntProperty()

    def draw(self, context):
        col = self.layout.column()
        about = col.box()
        about.label(text="Gothic 3 ImpExp by UniLinkIF — GPL-3.0-or-later with additional terms (NOTICE)", icon="INFO")
        about.operator("wm.url_open", text="github.com/UniLinkIF/gothic3-impexp", icon="URL").url = "https://github.com/UniLinkIF/gothic3-impexp"
        col.prop(self, "game_dir")
        col.prop(self, "core_exe")
        col.prop(self, "cache_dir")
        col.prop(self, "mods_dir")
        col.prop(self, "author")
        col.separator()
        col.label(text="Свої теки текстур (PNG, TGA, DDS; з підтеками): перекривають текстури гри при імпорті,")
        col.label(text="а при експорті знаходять зображення, чий файл переїхав")
        row = col.row()
        row.template_list("G3_UL_TextureDirs", "", self, "texture_dirs", self, "texture_dir_index", rows=3)
        ops = row.column(align=True)
        ops.operator("gothic3.texture_dir_add", icon="ADD", text="")
        ops.operator("gothic3.texture_dir_remove", icon="REMOVE", text="")


def prefs(context=None):
    context = context or bpy.context
    return context.preferences.addons[ADDON_ID].preferences


def texture_dirs():
    """The texture folders from the preferences that exist, as absolute paths."""
    import os
    return [d for d in (os.path.normpath(bpy.path.abspath(t.path)) for t in prefs().texture_dirs if t.path) if os.path.isdir(d)]


_classes = (G3_PG_TextureDir, G3_UL_TextureDirs, G3_OT_TextureDirAdd, G3_OT_TextureDirRemove, G3ImpExpPreferences)


def register():
    for c in _classes:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
