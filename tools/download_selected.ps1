# Downloads the 3D models Kord's device sprites are rendered from (see
# crates/kord/assets/sprites/CREDITS.md). The .glb files are not in git:
# the app only embeds the rendered PNGs, so you need them only to re-render.
#
# Needs your Sketchfab API token (https://sketchfab.com/settings/password):
#   $env:SKETCHFAB_TOKEN = "<your token>"; .\tools\download_selected.ps1
$models = @(
    # Folder under assets/model, Sketchfab uid, file name; author in comments.
    @("AirpodsPro", "a12133c2ff014c3b80ec0172e387fcea", "buds"),                            # pichou
    @("AirpodsPro", "2a97b522bd1e43ffa342e4a19ff21e35", "case"),                            # eltayerkebulan
    @("AirPods4", "e44730974131402eb496352cab19c82e", "airpods-4"),                         # Falah3D
    @("AirPodsMax", "3d0643db08fc4ef89bda55a74077ece4", "airpods-max-sky-blue"),            # polyman Studio
    @("BeatsStudio", "069a29ee35364846929b9ed03841d30c", "beats-studio"),                   # IMPACTRA
    @("GalaxyBudsLive", "7e4a0f2202144b91b4c47ffbfdf94584", "samsung-galaxy-buds-live"),    # DatSketch
    @("SonyWH1000XM5", "5d8aea0a780b49fa89c9c205912414e3", "sony-wh-1000xm5"),              # Zalaneke
    @("JblTune720BT", "9b5791b07d8b44deb46212ed450567e9", "jbl-tune-720bt"),                # polyman Studio
    @("JabraEliteBuds", "4bce450609f7470190ebc1d4cf5e57ef", "jabra-elite-buds"),            # Sam De Lara
    # No Zone Vibe is downloadable; the G435 has the same lightweight shape.
    @("LogitechG435", "57209e050bd047769c237f46516f5227", "headphones-logitech-g435"),      # seiigiru
    # Unbranded, for headphones without art of their own.
    @("GenericHeadphones", "3da4c7788c314b069b42cace0cccede0", "wireless-headphones"),      # Octaclee
    @("GenericEarbuds", "d05c7996482f451ab405f9323e22eece", "tws-buds-low-poly-sci-fi-blender")  # Himesh Singh
)
if (-not $env:SKETCHFAB_TOKEN) { Write-Error "Set `$env:SKETCHFAB_TOKEN first."; exit 1 }
foreach ($model in $models) {
    $folder, $uid, $name = $model
    # Already downloaded: keep it.
    if (Test-Path "assets/model/$folder/$name.glb") { continue }
    python tools/find_models.py --download $uid --to "assets/model/$folder" --name $name
}
