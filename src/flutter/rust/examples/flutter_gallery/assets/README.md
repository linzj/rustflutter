# Where these came from

Not authored here. Every file is upstream's, copied so that the port shows the
same pictures rather than stand-ins.

| Path | Source |
|---|---|
| `studies/*_card*.png` | `flutter_gallery_assets` 1.0.2, `lib/assets/studies/` |
| `icons/{material,cupertino,reference}.png` | `flutter_gallery_assets` 1.0.2, `lib/assets/icons/` |
| `fonts/GalleryIcons.ttf` | `flutter_gallery_assets` 1.0.2, `lib/fonts/` |
| `fonts/MaterialIcons-Regular.otf` | the Flutter SDK, `bin/cache/artifacts/material_fonts/` |
| `shrine/*.jpg` (38 photographs) | `shrine_images` 2.0.2, from pub.dev |

`flutter_gallery_assets` is BSD-licensed, the same terms as Flutter itself.
Material Icons and `shrine_images` are both Apache 2.0 -- a different license
from the rest of this repository, which is why the root `NOTICE` names them
and `LICENSES/Apache-2.0.txt` carries the text.

They are baked into the binary with `include_bytes!` rather than loaded from
disk, because there is no asset bundle here -- no `AssetManager`, no
`pubspec.yaml` manifest, no `flutter_assets` directory next to the executable.
Compiling them in is what a single-file native binary can do without inventing
one.

`MaterialIcons-Regular.otf` is 1.6 MB, larger than everything else put together.
It earns it: four demo rows take their icon from it rather than from
`GalleryIcons`, and so does every piece of chrome -- the back arrow, the
settings gear, the chevrons on a category header.
