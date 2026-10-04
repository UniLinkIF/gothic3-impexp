"""Add-on preferences: where Gothic 3 is, where the core and the cache are."""

import bpy
from bpy.props import StringProperty

ADDON_ID = __package__


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

    def draw(self, context):
        col = self.layout.column()
        col.prop(self, "game_dir")
        col.prop(self, "core_exe")
        col.prop(self, "cache_dir")
        col.prop(self, "mods_dir")


def prefs(context=None):
    context = context or bpy.context
    return context.preferences.addons[ADDON_ID].preferences


def register():
    bpy.utils.register_class(G3ImpExpPreferences)


def unregister():
    bpy.utils.unregister_class(G3ImpExpPreferences)
