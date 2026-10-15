---
description: Self-contained device canvases, image assets, drawing expressions and programmable parameter controls
read_when: custom SDK control, device background, icon, photo, texture, XY pad, drawing API, image asset
type: spec
status: current
last_verified: 2026-10-15
covers:
  - crates/device-sdk/src/ui.rs
  - crates/ui/src/device_canvas.rs
  - crates/ui/src/detail_view.rs
  - crates/device-sdk/examples/custom_panel.rs
---

# Custom device panels

`device.ui` optionally describes a canvas. It contains size, background RGBA, embedded image
assets, drawing layers, custom controls and a list of parameters hidden from the stock panel.
Everything is stored in the single JSON `.sd` and then embedded in the project. Old files
without a canvas use their existing controls. No external paths or executable UI code are read
from a device file. Rust authors program panels by constructing the SDK's drawing expressions
and input bindings. Normalized expressions can select logarithmic mapping explicitly. Standard controls remain available below the canvas.

Canvas `size: [width, height]` defaults to `[640, 240]`. Optional
`viewport_size: [width, height]` sets the preferred scrollable device body independently of the
canvas, including node rows and stock controls. Both dimensions use logical pixels in 24..8192.
The host adds a 20-pixel header and 6-pixel padding on each side. Available host space clips the
viewport; oversized contents scroll without scaling. This works in row and stacked layouts.
Omitted or null `viewport_size` preserves automatic sizing: row width is at least 220 or canvas
width plus 12, row height fills the detail area; stacked width fills the column (normally 260),
and height fits node rows, canvas and stock controls. Setting `size` does not scale artwork.

Canvas size is 24..8192 logical pixels per axis. Up to 128 custom controls and 1024 total drawing
commands are accepted. Each control has a unique id, accessible name, tooltip, bounds, drawings,
a gesture and optional X/Y parameter bindings. Bounds stay inside the canvas. Drawing positions
are local to the canvas or the control. Layer order is array order; controls draw after layers.

Drawing commands: rectangle, circle, line, path, arc, text, image and four-corner gradient.
Paths have at most 256 points; filled paths must be convex. Arc tessellation is bounded. Colours are RGBA byte quadruples.
Text can append a parameter's current value with bounded decimal precision. Numeric drawing
values can be literals or bounded expressions (parameter, normalized parameter, add, multiply,
sine and cosine). Expression depth is at most 16, with 32768 expression nodes per canvas;
non-finite results become zero, finite values are bounded before drawing. This supports moving
markers, response drawings, circular handles, segmented displays and meters driven by parameters.
No DSP outputs or audio-thread buffers are read by the UI.

Image assets use stable ids and base64 PNG/JPEG bytes. File size is at most 2 MiB; compressed image
bytes before base64 total at most 1 MiB, one asset at most 512 KiB, decoded dimensions at most 4096 per axis,
one image at most 1 million pixels and total images at most 4 million pixels. Header dimensions
are validated before decoding; image decoding uses allocation limits off the audio thread.
Decode failures appear in the panel as an asset error and preserve the graph. Textures and parsed
UI graphs are cached with bounded retention. Supported image layouts are stretch, contain and
cover, with UV crop, RGBA tint and optional tile size. Repetition uses texture wrapping, not an
unbounded loop of drawing operations. Vector icons are paths/lines/circles, raster icons can
use PNG transparency. Photos and patterns are original assets supplied by the author.

Gestures: drag (relative X/Y), position (absolute pointer position, Y upward), toggle, momentary.
Bindings map normalized coordinates to parameter units with optional logarithmic mapping.
Keyboard arrows edit axes, Shift provides fine adjustment, double click resets defaults;
Space/Enter activate buttons. The control name and values are exposed to accessibility.
Edits dispatch `device.setParam` with a shared gesture id so a two-axis drag is one undo gesture.
All bindings reference declared parameter ids, and values use their range/discrete clamping.

A canvas may hide stock parameter rows with `hidden_parameters`; this only changes presentation.
All hidden values remain available through commands and project state. UI resources do not run
inside the DSP callback. UI code never loads a user native library or script.

## Serialized drawing fields

All drawings have a `kind`. Rectangles are `[x,y,width,height]`, points are `[x,y]`; geometry
accepts `UiValue` numbers or expression objects. Colours are `[r,g,b,a]` in 0..255.

| kind | Required fields | Optional fields |
| --- | --- | --- |
| `rect` | `rect`, `color` | `radius` (0) |
| `circle` | `center`, `radius`, `color` | |
| `line` | `from`, `to`, `color` | `width` (1) |
| `path` | `points`, `color` | `width` (1), `closed` (false), `fill` (none, convex) |
| `arc` | `center`, `radius`, `start`, `end`, `color` | `width` (1); angles in radians |
| `text` | `position`, `text`, `color`, `size` | `parameter` (none), `decimals` (0) |
| `image` | `asset`, `rect` | `uv` ([0,0,1,1]), `tint` (white), `tile_size` (none), `fit` (`stretch`) |
| `gradient` | `rect`, `colors` | Colours ordered top-left, top-right, bottom-right, bottom-left |

Stroke widths and rectangle corner radii are 0..128; text size is 8..48. Image `fit` also accepts
`contain` and `cover`. `tile_size` overrides fit with repetition. `text.parameter` appends the
current value and unit to the prefix `text`. `decimals` is 0..8. Unknown fields are rejected.

A value expression uses `op`: `parameter` has `parameter` (id), `normalized` (false), `logarithmic`
(false); `add` and `multiply` have `a` and `b`; `sin` and `cos` have `value` (radians).
The parameter expression returns raw units unless `normalized` is true. Use the same
`logarithmic` flag on bindings and normalized drawings.

A serialized control referencing a declared continuous `cutoff` parameter:

```json
{
  "id":"frequency", "name":"Cutoff", "tooltip":"Drag vertically",
  "bounds":[16,16,100,100], "gesture":"drag",
  "y":{"parameter":"cutoff","logarithmic":true},
  "drawing":[
    {"kind":"rect","rect":[0,0,100,100],"radius":8,"color":[18,30,42,255]},
    {"kind":"text","position":[8,40],"text":"","parameter":"cutoff",
     "decimals":0,"color":[120,230,190,255],"size":14}
  ]
}
```

`x`/`y` bindings default to none; `tooltip` and `drawing` default to empty; `gesture` defaults to
`drag`. `id`, `name`, and `bounds` are required. A canvas defaults to 640x240 with background
[24,28,34,255], empty assets/layers/controls/hidden_parameters. Asset objects require `id` and
`data` (standard base64). Runtime controls bind only declared parameters, never paths or commands.
