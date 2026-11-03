# Renders a glTF model to a transparent PNG sprite with studio lighting.
#
# Kord never loads 3D models at runtime: sprites are rendered once with this
# script, committed under crates/kord/assets/sprites/, and embedded in the
# binary. That keeps the app small while still showing realistic art.
#
# Usage:
#   blender --background --python tools/render_model.py -- model.glb out.png \
#       [--size 480x360] [--yaw 25] [--pitch 18] [--roll 0] [--samples 96] \
#       [--hide name,name] [--only name,name] [--light 1.0] [--debug-bg]
import argparse
import math
import os
import sys

import bpy
from mathutils import Vector

args = argparse.ArgumentParser()
args.add_argument("model")
args.add_argument("output")
args.add_argument("--size", default="480x360")
args.add_argument("--yaw", type=float, default=25.0, help="degrees around the vertical axis")
args.add_argument("--pitch", type=float, default=18.0, help="degrees above the horizon")
args.add_argument("--roll", type=float, default=0.0, help="degrees to turn the image")
args.add_argument("--margin", type=float, default=1.08, help="framing margin around the model")
args.add_argument("--samples", type=int, default=96)
args.add_argument("--hide", default="", help="comma-separated substrings of object names to leave out")
args.add_argument("--only", default="", help="comma-separated substrings: render only matching objects")
args.add_argument("--light", type=float, default=1.0, help="multiplier for all lights")
args.add_argument("--debug-bg", action="store_true", help="grey background instead of transparency")
opts = args.parse_args(sys.argv[sys.argv.index("--") + 1 :])
width, height = (int(v) for v in opts.size.lower().split("x"))


def matches(name, patterns):
    return any(p.lower() in name.lower() for p in patterns.split(",") if p)


bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=opts.model)
scene = bpy.context.scene

meshes = []
for obj in scene.objects:
    if obj.type != "MESH":
        continue
    if matches(obj.name, opts.hide) or (opts.only and not matches(obj.name, opts.only)):
        obj.hide_render = True
        continue
    meshes.append(obj)

# Bounding box of everything visible, in world space.
corners = [obj.matrix_world @ Vector(c) for obj in meshes for c in obj.bound_box]
low = Vector((min(c.x for c in corners), min(c.y for c in corners), min(c.z for c in corners)))
high = Vector((max(c.x for c in corners), max(c.y for c in corners), max(c.z for c in corners)))
center = (low + high) / 2
radius = (high - low).length / 2

# Camera on a sphere around the model, looking at its centre.
yaw, pitch = math.radians(opts.yaw), math.radians(opts.pitch)
direction = Vector((math.sin(yaw) * math.cos(pitch), -math.cos(yaw) * math.cos(pitch), math.sin(pitch)))
camera_data = bpy.data.cameras.new("camera")
camera_data.type = "ORTHO"
camera = bpy.data.objects.new("camera", camera_data)
scene.collection.objects.link(camera)
camera.location = center + direction * radius * 6
camera.rotation_euler = (-direction).to_track_quat("-Z", "Y").to_euler()
camera.rotation_euler.rotate_axis("Z", math.radians(opts.roll))
camera_data.clip_start = radius * 0.01
camera_data.clip_end = radius * 20
scene.camera = camera
bpy.context.view_layer.update()

# Fit the model's projected extent into the frame.
view = camera.matrix_world.inverted()
projected = [view @ c for c in corners]
min_x, max_x = min(p.x for p in projected), max(p.x for p in projected)
min_y, max_y = min(p.y for p in projected), max(p.y for p in projected)
aspect = width / height
camera_data.ortho_scale = max(max_x - min_x, (max_y - min_y) * aspect) * opts.margin
camera_data.sensor_fit = "HORIZONTAL"
camera_data.shift_x = ((min_x + max_x) / 2) / camera_data.ortho_scale
camera_data.shift_y = ((min_y + max_y) / 2) / camera_data.ortho_scale


def area_light(name, energy, size, offset):
    data = bpy.data.lights.new(name, "AREA")
    data.energy = energy * opts.light * radius * radius * 60
    data.size = radius * size
    light = bpy.data.objects.new(name, data)
    scene.collection.objects.link(light)
    light.location = center + Vector(offset) * radius * 4
    light.rotation_euler = (center - light.location).to_track_quat("-Z", "Y").to_euler()


# Soft product-shot lighting: big key from the front-left, fill from the
# right, rim from behind, on a bright environment.
area_light("key", 6.0, 4.0, (-1.2, -1.6, 1.4))
area_light("fill", 2.5, 5.0, (1.6, -1.0, 0.5))
area_light("rim", 4.0, 3.0, (0.4, 1.8, 1.2))
area_light("top", 2.0, 6.0, (0.0, 0.0, 2.0))

world = bpy.data.worlds.new("world")
world.use_nodes = True
nodes, links = world.node_tree.nodes, world.node_tree.links
environment = nodes["Background"]
environment.inputs[0].default_value = (1.0, 1.0, 1.0, 1.0)
environment.inputs[1].default_value = 0.3 * opts.light
if opts.debug_bg:
    # A grey backdrop seen only by the camera, so the lighting stays the same.
    backdrop = nodes.new("ShaderNodeBackground")
    backdrop.inputs[0].default_value = (0.22, 0.22, 0.24, 1.0)
    is_camera = nodes.new("ShaderNodeLightPath")
    mix = nodes.new("ShaderNodeMixShader")
    links.new(is_camera.outputs["Is Camera Ray"], mix.inputs[0])
    links.new(environment.outputs[0], mix.inputs[1])
    links.new(backdrop.outputs[0], mix.inputs[2])
    links.new(mix.outputs[0], nodes["World Output"].inputs[0])
scene.world = world

scene.render.engine = "CYCLES"
scene.cycles.samples = opts.samples
scene.cycles.use_denoising = True
scene.render.film_transparent = not opts.debug_bg
scene.render.resolution_x = width
scene.render.resolution_y = height
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = "PNG"
scene.render.image_settings.color_mode = "RGBA"
scene.render.image_settings.compression = 100
scene.view_settings.view_transform = "Standard"
scene.view_settings.look = "None"
# Blender resolves relative paths against the drive root on Windows.
scene.render.filepath = os.path.abspath(opts.output)
bpy.ops.render.render(write_still=True)
print(f"RENDERED {opts.output}")
