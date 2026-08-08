# Asset notice

## License scope

The MIT License in `LICENSE` applies to the project's source code and
project-authored textual documentation. Raster artwork has the family-specific
license scope and provenance described below.

## Cockroach artwork

The following files form one related artwork family:

- `bugs/cockroach/cockroach_parts_atlas.png`
- `bugs/template/atlas.png`
- `packaging/cockroach.ico`
- `docs/screenshots/windows11-desktop-pet.png`, insofar as it reproduces
  the cockroach artwork

The development record identifies the base as a 1024×1536 top-down cockroach
image supplied by the user during development under the local, untracked path
`build-asan/assets/cockroach_body.png` (SHA-256:
`53ae332632b873251918a431d8f8d374b4001650eeb3ecdb0d041130422379cf`).
The runtime atlas and icon were produced through generated edits, background
removal, component extraction, rearrangement, compositing, and resizing based
on that material.

No upstream author, source URL, or license for the supplied base image was
recorded, and those rights have not been independently verified. Accordingly,
these artwork files are not offered under the repository's MIT License. Their
inclusion does not represent that they are public domain, open source, or
otherwise available for unrestricted reuse. Recipients are responsible for
establishing any permissions required for use or redistribution.

The Windows desktop screenshot also contains operating-system user interface
elements. Rights in those elements remain with their respective owners. No
affiliation or endorsement is implied.

For unambiguous open-source redistribution, replace this artwork family with
assets whose provenance and redistribution license are documented.

## Turtle artwork

The following project-authored artwork was produced for this repository using
OpenAI's built-in image generation tool, followed by chroma-key removal,
component masking, atlas composition, and icon resizing:

- `bugs/turtle/turtle_parts_atlas.png`
- `packaging/turtle.ico`
- `docs/screenshots/windows11-turtle-pet.png`, insofar as it reproduces the
  turtle artwork

The generation brief requested an exact orthographic top-down, friendly
juvenile land tortoise rendered as polished semi-realistic 3D game art on a
flat chroma background. The checked-in `bugs/turtle/ARTWORK.md` records the
asset layout and workflow. To the extent the project contributors hold
copyright or other licensable rights in this generated and edited artwork,
those rights are offered under the repository's MIT License.
