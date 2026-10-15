"""Supplement `forge views` with rear and shoulder crops, without editing a body.

Run with Blender's background interpreter, after `--` passing a GLB and an
output directory. Uses CPU Cycles so a failed CUDA preflight does not prevent
inspection of existing assets. These images are review evidence, not exports.
"""

import hashlib
import json
from pathlib import Path
import sys

import bpy
from mathutils import Vector


def aim(obj, target):
    obj.rotation_euler = (Vector(target) - obj.location).to_track_quat("-Z", "Y").to_euler()


source, output = (Path(arg).resolve() for arg in sys.argv[sys.argv.index("--") + 1:])
output.mkdir(parents=True, exist_ok=True)
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=str(source))
bpy.context.view_layer.update()

# glTF Y-up is Blender Z-up after import. Frame both raw lifts and fitted bodies
# at 1.8 m for comparable inspection without changing any mesh data or normals.
meshes = [obj for obj in bpy.context.scene.objects
          if obj.type == "MESH" and obj.visible_get() and not obj.hide_render]
points = [obj.matrix_world @ Vector(corner) for obj in meshes for corner in obj.bound_box]
lower = Vector(tuple(min(point[axis] for point in points) for axis in range(3)))
upper = Vector(tuple(max(point[axis] for point in points) for axis in range(3)))
scale = 1.8 / (upper.z - lower.z)
offset = Vector(((upper.x + lower.x) / 2, (upper.y + lower.y) / 2, lower.z))
roots = [obj for obj in bpy.context.scene.objects if obj.parent is None]
framing = bpy.data.objects.new("Inspection framing only", None)
bpy.context.collection.objects.link(framing)
for obj in roots:
    obj.parent = framing
framing.scale = (scale,) * 3
framing.location = -offset * scale

scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.device = "CPU"
scene.cycles.samples = 24
scene.cycles.use_denoising = True
scene.render.threads_mode = "FIXED"
scene.render.threads = 8
scene.render.resolution_x = 768
scene.render.resolution_y = 768
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = "PNG"
scene.world = bpy.data.worlds.new("Inspection world")
scene.world.use_nodes = True
scene.world.node_tree.nodes["Background"].inputs[0].default_value = (0.28, 0.28, 0.28, 1)
scene.world.node_tree.nodes["Background"].inputs[1].default_value = 0.6

for index, location in enumerate(((3, 4, 5), (-3, 2, 3), (0, -4, 4))):
    light = bpy.data.lights.new(f"Inspection light {index}", "AREA")
    light.energy = 450
    light.shape = "DISK"
    light.size = 3
    obj = bpy.data.objects.new(light.name, light)
    scene.collection.objects.link(obj)
    obj.location = location
    aim(obj, (0, 0, 1))

camera = bpy.data.objects.new("Inspection camera", bpy.data.cameras.new("Inspection camera"))
scene.collection.objects.link(camera)
scene.camera = camera
camera.data.type = "ORTHO"
views = [
    ("rear-three-quarter", (2.4, 4, 2), (0, 0, 0.92), 2.1),
    ("overhead-shoulders", (0.7, 2.3, 3.8), (0, 0, 1.47), 0.95),
    ("rear-seat", (0.3, 4, 1.5), (0, 0, 0.94), 0.70),
    ("rear-neck", (0, 4, 2.2), (0, 0, 1.50), 0.90),
]
records = []
for name, location, focus, span in views:
    camera.location = location
    camera.data.ortho_scale = span
    aim(camera, focus)
    path = output / f"{name}.png"
    scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)
    records.append(dict(name=name, location=location, focus=focus, span_m=span,
                        image=path.name, sha256=hashlib.sha256(path.read_bytes()).hexdigest()))

(output / "review.json").write_text(json.dumps(dict(
    purpose="Inspection only; source mesh, skin, normals and materials unchanged",
    source=str(source), source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
    renderer=f"Blender {bpy.app.version_string} CPU Cycles", samples=24,
    script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    framing_height_m=1.8, views=records,
), indent=2) + "\n")
