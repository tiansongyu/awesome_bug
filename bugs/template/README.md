# Bug species template

Copy this directory, rename it, and edit the three self-contained files:

- `manifest.lua` describes the species, atlas, body collider, capabilities,
  render tint, and named sprite parts.
- `behavior.lua` creates one independent controller per bug. Its `step` method
  returns motion intent; its `pose` method returns body and named-part poses.
- `atlas.png` contains every sprite rectangle declared by the manifest.

The runtime loads `manifest.lua` as data. Paths must be relative files inside
the species directory, part names must be unique, and atlas rectangles must be
in bounds. A new species never requires native subclassing or a main-loop
change.

Behavior ABI v1 receives only validated data and a read-only `host` table:

```lua
local controller = host.fsm.create(definition, "moving", context)
local sample = host.random("species.purpose", low, high)
```

Every random draw needs a stable, descriptive tag. Do not use module-level
mutable instance state, `math.random`, file/system APIs, `require`, or debug
hooks. The sandbox enforces a memory budget and instruction limit.

Keep policy in Lua: states, timers, targets, speed, turn, gait, antennae, and
other organs. The native runtime owns only generic mechanisms such as Windows
input, work-area bounds, continuous obstacle collision, resource loading, and
rendering.

Before packaging a species, run the runtime contract tests on both the native
test host and the Windows target. The included template alternates between
moving and resting and is intentionally small enough to use as a fixture.
