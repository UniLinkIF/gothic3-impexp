"""Frame effects of a Gothic 3 clip (footsteps, hit sounds, …): kept on the action as the custom property
`g3_frame_effects`, a dict of Blender frame -> effect name, shown and edited in the Gothic 3 panel. Motion export
writes them into the clip (the game keeps them per key frame)."""

import bpy
from bpy.props import StringProperty

PROP = "g3_frame_effects"


def _fps(scene):
    return scene.render.fps / scene.render.fps_base


def store(scene, action, effects):
    """`effects` = [(time s, name)] from the core."""
    start = action.frame_range[0]
    action[PROP] = {str(int(round(start + t * _fps(scene)))): n for t, n in effects}


def as_times(scene, action):
    """The action's effects as [(time s, name)], or None when it has none of its own (the clip's stay)."""
    if PROP not in action:
        return None
    start = action.frame_range[0]
    return sorted(((int(f) - start) / _fps(scene), n) for f, n in action[PROP].items())


def _action(context):
    o = context.active_object
    return o.animation_data.action if o and o.type == "ARMATURE" and "g3_actor" in o and o.animation_data else None


def _known(self, context, edit_text):
    names = sorted({n for a in bpy.data.actions if PROP in a for n in a[PROP].values()})
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
        fx[str(context.scene.frame_current)] = self.name
        a[PROP] = fx
        return {"FINISHED"}


class G3_OT_effect_remove(bpy.types.Operator):
    bl_idname = "gothic3.effect_remove"
    bl_label = "Прибрати ефект"
    bl_options = {"REGISTER", "UNDO"}

    frame: StringProperty()

    def execute(self, context):
        a = _action(context)
        if a is not None and PROP in a:
            fx = dict(a[PROP])
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
        r = box.row()
        r.label(text=f"{f}: {fx[f]}")
        r.operator("gothic3.effect_remove", text="", icon="X").frame = f


_classes = (G3_OT_effect_add, G3_OT_effect_remove)


def register():
    for c in _classes:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
