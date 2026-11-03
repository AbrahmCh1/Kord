# Lists the objects of a glTF model, for deciding how to frame renders.
# Usage: blender --background --python tools/inspect_model.py -- model.glb
import sys

import bpy

path = sys.argv[sys.argv.index("--") + 1]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=path)
for obj in bpy.context.scene.objects:
    parent = obj.parent.name if obj.parent else "-"
    dims = tuple(round(d, 4) for d in obj.dimensions)
    loc = tuple(round(v, 4) for v in obj.matrix_world.translation)
    mats = [s.material.name for s in getattr(obj, "material_slots", []) if s.material]
    print(f"OBJ {obj.type:6} {obj.name!r:40} parent={parent!r:30} dims={dims} loc={loc} mats={mats}")
