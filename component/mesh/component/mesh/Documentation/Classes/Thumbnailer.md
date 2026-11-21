# Thumbnailer

`cs.MeshPreview.Thumbnailer.new({target : Text})` creates the deterministic
STL/PLY thumbnail rendering feature (spec §22). The namespace is configured
independently of the repository/component name; within the component the class
is `cs.Thumbnailer`.

`target` may be omitted on macOS (universal). On Windows, explicitly pass
`"Windows-x64"`. `"macOS-universal"` is also accepted on macOS. Other target
values produce `unsupported_architecture`; no processor-name heuristic is used.

| Method | Result |
| --- | --- |
| `render(input : 4D.File; output : 4D.File; options : Object) : Object` | Renders `input` to `output` (PNG); `value` is the mesh-thumb render envelope result |
| `inspect(input : 4D.File) : Object` | Geometry metadata; `value` is the inspect envelope result |
| `version() : Object` | Plain CLI version text parsed into `value:{name,version}` |
| `capabilities() : Object` | CLI capabilities envelope; `value` is the capabilities result |

`render` options (all optional): `size` (positive integer, default 64),
`view` (`iso`, `top`, `front` or `side`), `background` (`#RRGGBB` or
`transparent`) and `quality` (integer 1–8). Invalid option values fail with
`invalid_argument` before any process is launched. `clearCache` is not
implemented; mesh-thumb renders without a persistent cache.

All methods are synchronous, close stdin and enforce the 30-second process
timeout. Success is `{ok:true, stdout:Text, stderr:Text, exitCode:0,
value:Object}`. `render` additionally validates the returned PNG header before
exposing success and reports `pngValid:true` plus the deterministic output
size `pngSize`. stdout carries the mesh-thumb JSON envelope untouched; `value`
is its `result` member. `version` stdout is the plain text
`mesh-thumb 0.3.0\n` and `value` is parsed from it.

Failures have `ok:false` and `error:{code,message{,path,stage,details}}`.
`render`/`inspect`/`capabilities` decode the stdout envelope even when the
process exits nonzero (exit codes are advisory; the envelope is
authoritative), mapping `{"schema_version":1,"success":false,"error":{...}}`
to `error.code/message/path/stage`. Non-envelope launch/process failures keep
`missing_executable`, `unsupported_platform`, `unsupported_architecture`,
`launch_failed`, `process_error`, `process_terminated`, `cli_error`. JSON
payloads that are not objects, or whose `schema_version` is not 1, fail with
`invalid_protocol`. `render` reports `invalid_png` when the CLI claims success
but `output` is not a valid PNG. Unicode, spaces, quotes, backslashes,
newlines and shell metacharacters in paths are literal data, never shell
instructions.

```4d
var $tool : cs.MeshPreview.Thumbnailer
var $target : Text
var $options : Object
var $input; $output : 4D.File
$target:="macOS-universal"
If (Is Windows)
  $target:="Windows-x64"
End if
$tool:=cs.MeshPreview.Thumbnailer.new($target)
$input:=Folder(fk resources folder).file("triangle.stl")
$output:=Folder(fk temporary folder).file("render.png")
$options:=New object("size"; 256; "view"; "iso"; "background"; "#FFFFFF")
$result:=$tool.render($input; $output; $options)
If ($result.ok)
  ALERT("Rendered "+String($result.pngSize)+" bytes")
  $meta:=$tool.inspect($input)
End if
```

`_decode` and `_cli` are implementation details; underscore names are not an
access-control boundary in 4D.