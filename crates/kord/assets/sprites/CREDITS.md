# Sprite credits

The images in this folder are renders of the following 3D models, made with
`tools/render_model.py` (Blender, studio lighting, transparent background).
The models themselves live in `assets/model/`.

| Sprite | Model | Author | License |
|---|---|---|---|
| `airpods-pro/buds.png` | [Apple Air Pods New](https://sketchfab.com/3d-models/apple-air-pods-new-a12133c2ff014c3b80ec0172e387fcea) | pichou | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `airpods-pro/case.png` | [AirPods Pro](https://skfb.ly/pwDNR) | eltayerkebulan | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `airpods/set.png` | [AirPods 4](https://sketchfab.com/3d-models/e44730974131402eb496352cab19c82e) | Falah3D | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `airpods-max/headphones.png` | [Airpods max sky blue](https://sketchfab.com/3d-models/3d0643db08fc4ef89bda55a74077ece4) | polyman Studio | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `beats/headphones.png` | [Beats Studio](https://sketchfab.com/3d-models/069a29ee35364846929b9ed03841d30c) | IMPACTRA | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `galaxy-buds/case.png` | [Samsung Galaxy Buds Live](https://sketchfab.com/3d-models/7e4a0f2202144b91b4c47ffbfdf94584) | DatSketch | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `sony/headphones.png` | [Sony Wh-1000xm5](https://sketchfab.com/3d-models/5d8aea0a780b49fa89c9c205912414e3) | Zalaneke | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `jbl/headphones.png` | [JBL Tune 720BT](https://sketchfab.com/3d-models/9b5791b07d8b44deb46212ed450567e9) | polyman Studio | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `jabra/buds.png` | [Jabra Elite Buds](https://sketchfab.com/3d-models/4bce450609f7470190ebc1d4cf5e57ef) | Sam De Lara | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `generic/headphones.png` | [Wireless Headphones](https://sketchfab.com/3d-models/3da4c7788c314b069b42cace0cccede0) | Octaclee | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `generic/earbuds.png` | [TWS buds](https://sketchfab.com/3d-models/d05c7996482f451ab405f9323e22eece) | Himesh Singh | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |
| `logitech/headphones.png` | [Headphones Logitech G435](https://sketchfab.com/3d-models/57209e050bd047769c237f46516f5227) | seiigiru | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/) |

Changes: rendered to 2D images from a fixed camera angle, shown in greyscale; the
Galaxy Buds logo was left out of the render.

Product names are trademarks of their owners. Kord is not affiliated with any of them.
Models were found with `tools/find_models.py`.

To re-render:

```
blender --background --python tools/render_model.py -- assets/model/AirpodsPro/buds.glb crates/kord/assets/sprites/airpods-pro/buds.png --size 360x270 --samples 128 --yaw 0 --pitch 10 --light 1.25
blender --background --python tools/render_model.py -- assets/model/AirpodsPro/case.glb crates/kord/assets/sprites/airpods-pro/case.png --size 300x234 --samples 128 --yaw 200 --pitch 15 --light 1.25
blender --background --python tools/render_model.py -- assets/model/AirPods4/airpods-4.glb crates/kord/assets/sprites/airpods/set.png --size 300x300 --samples 128 --light 1.25 --yaw 15 --pitch 14
blender --background --python tools/render_model.py -- assets/model/AirPodsMax/airpods-max-sky-blue.glb crates/kord/assets/sprites/airpods-max/headphones.png --size 330x300 --samples 128 --light 1.25 --yaw 20 --pitch 10
blender --background --python tools/render_model.py -- assets/model/BeatsStudio/beats-studio.glb crates/kord/assets/sprites/beats/headphones.png --size 330x300 --samples 128 --light 2.4 --yaw 20 --pitch 4
blender --background --python tools/render_model.py -- assets/model/GalaxyBudsLive/samsung-galaxy-buds-live.glb crates/kord/assets/sprites/galaxy-buds/case.png --size 300x210 --samples 128 --light 2.2 --yaw 15 --pitch 12 --hide Logo
blender --background --python tools/render_model.py -- assets/model/SonyWH1000XM5/sony-wh-1000xm5.glb crates/kord/assets/sprites/sony/headphones.png --size 330x300 --samples 128 --light 2.6 --yaw 290 --pitch 10
blender --background --python tools/render_model.py -- assets/model/JblTune720BT/jbl-tune-720bt.glb crates/kord/assets/sprites/jbl/headphones.png --size 330x300 --samples 128 --light 2.6 --yaw 20 --pitch 10
blender --background --python tools/render_model.py -- assets/model/JabraEliteBuds/jabra-elite-buds.glb crates/kord/assets/sprites/jabra/buds.png --size 360x210 --samples 128 --light 2.4 --yaw 10 --pitch 10
blender --background --python tools/render_model.py -- assets/model/LogitechG435/headphones-logitech-g435.glb crates/kord/assets/sprites/logitech/headphones.png --size 330x300 --samples 128 --light 2.6 --yaw 20 --pitch 10
blender --background --python tools/render_model.py -- assets/model/GenericHeadphones/wireless-headphones.glb crates/kord/assets/sprites/generic/headphones.png --size 330x300 --samples 128 --light 2.6 --yaw 20 --pitch 10
blender --background --python tools/render_model.py -- assets/model/GenericEarbuds/tws-buds-low-poly-sci-fi-blender.glb crates/kord/assets/sprites/generic/earbuds.png --size 300x270 --samples 128 --light 1.25 --yaw 20 --pitch 22
```
