"""What the game has, sorted into the categories the import dialogs offer: models by the archive folder Gothic 3
keeps them in, actors by their rig, clips by the words of their names. Lists are read once per session."""

from . import core

ALL = "all"

MESH_CATEGORIES = (
    ("items", "Предмети", "Зброя, обладунки, трави, трофеї, речі"),
    ("buildings", "Будівлі", "Будинки, вежі, руїни"),
    ("decor", "Декор і меблі", "Меблі, бочки, ящики, інтерактивні об'єкти"),
    ("plants", "Рослини", "Кущі, трава, гілки"),
    ("locations", "Локації", "Міста, табори повстанців, підземелля, храми"),
    ("landscape", "Рельєф", "Шматки землі островів (і їх LOD)"),
    ("water", "Вода", "Море, океан, водоспади"),
    ("technical", "Службові", "Низькополігональні копії, допоміжні об'єкти редактора й фізики"),
)


def mesh_category(folder):
    f = folder.lower()
    if f.startswith("g3_items_"):
        return "items"
    if "lowpoly" in f or "depthmesh" in f or "editsupporter" in f or "physicsupporter" in f:
        return "technical"
    if "water" in f or "_sea_" in f or "_ocean_" in f:
        return "water"
    if "_brushes_" in f:
        return "plants"
    if "_buildings_" in f or "_ruins_" in f:
        return "buildings"
    if f.startswith("g3_objects_"):
        return "decor"
    if "_landscape_01" in f and "locations" not in f:
        return "landscape"
    return "locations"


ACTOR_CATEGORIES = (
    ("human", "Люди: тіло", "Тіла людей (G3_Hero_Body_*, товсті, раби)"),
    ("head", "Люди: голова", "Голови людей"),
    ("monster", "Тварини й монстри", "Вовки, орки, троллі, дракони…"),
    ("object", "Предмети й об'єкти", "Луки, двері, скрині з анімацією"),
)


def actor_category(name):
    n = name.lower()
    if n.startswith("g3_head_"):
        return "head"
    if n.startswith(("g3_hero_", "g3_fat_", "g3_slave_")) and "skeleton" not in n:
        return "human"
    if n.startswith(("it_", "mike_", "g3_door", "g3_chest", "g3_templedoor", "g3_barbecue", "g3_waterpipe", "g3_grindstone", "g3_orcboulder", "g3_weapons")):
        return "object"
    return "monster"


CLIP_CATEGORIES = (
    ("idle", "Стійка", "Ambient, стояння на місці"),
    ("move", "Рух", "Ходьба, біг, повороти"),
    ("attack", "Атаки", "Удари, атаки"),
    ("defend", "Захист", "Блоки, парирування, ухилення, отримання удару"),
    ("talk", "Діалоги й жести", "Розмова, жести"),
    ("sit", "Сидіння й лежання", "Сидіння, сон"),
    ("interact", "Взаємодія", "Робота з предметами й об'єктами"),
    ("death", "Смерть", "Падіння, смерть"),
    ("other", "Інше", ""),
)

_WORDS = (
    ("death", ("dead", "die", "death", "fall")),
    ("attack", ("attack", "hit_", "_hit", "raise", "power")),
    ("defend", ("parade", "parry", "block", "stumble", "dodge", "hurt")),
    ("move", ("move", "walk", "run", "turn", "jump", "swim", "sneak")),
    ("sit", ("sit", "sleep", "lie", "bed")),
    ("talk", ("talk", "say", "gesture", "listen", "dialog", "point")),
    ("idle", ("ambient", "stand")),
)


def clip_category(name):
    n = name.lower()
    for cat, words in _WORDS:
        if any(w in n for w in words):
            return cat
    return "interact" if "_use" in n or "interact" in n else "other"


_meshes = _actors = None


def meshes():
    global _meshes
    if _meshes is None:
        _meshes = [(n, f, mesh_category(f)) for n, f in core.run("find", core.game_dir(), "mesh")]
    return _meshes


def actors():
    global _actors
    if _actors is None:
        _actors = [(n, f, actor_category(n)) for n, f in core.run("find", core.game_dir(), "actor")]
    return _actors


def enum_items(categories):
    return [(ALL, "Усі", "")] + [(k, label, desc) for k, label, desc in categories]


def search(items, category, text, limit=300):
    words = text.lower().split()
    out = []
    for name, _, cat in items:
        if category != ALL and cat != category:
            continue
        l = name.lower()
        if all(w in l for w in words):
            out.append(name)
            if len(out) >= limit:
                break
    return out
