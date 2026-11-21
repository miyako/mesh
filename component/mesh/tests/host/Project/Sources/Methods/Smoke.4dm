//%attributes = {"invisible":true}
var $ignored : Integer
var $parameter; $target : Text
var $request; $result; $report; $options : Object
var $tool : cs.MeshPreview.Thumbnailer
var $requestFile; $statusFile : 4D.File
var $inputFile; $outputFile; $missingFile; $renamedExecutable : 4D.File
var $invalid : Variant
var $name : Text
var HOST_StatusFile : 4D.File
var HOST_Phase : Text
var HOST_PreviousHandler : Text
$ignored:=Get database parameter(User param value; $parameter)
If (Is Windows)
	$requestFile:=File($parameter; fk platform path)
Else
	$requestFile:=File($parameter; fk posix path)
End if
$request:=JSON Parse($requestFile.getText())
If (Is Windows)
	$statusFile:=File($request.status; fk platform path)
Else
	$statusFile:=File($request.status; fk posix path)
End if
$statusFile.setText("{\"success\":false,\"phase\":\"starting\"}")
HOST_StatusFile:=$statusFile
HOST_Phase:="constructor"
HOST_PreviousHandler:=Method called on error(ek local)
ON ERR CALL("Host_error"; ek local)
$target:="macOS-universal"
If (Is Windows)
	$target:="Windows-x64"
End if
$tool:=cs.MeshPreview.Thumbnailer.new($target)
$report:=New object("success"; True; "finished"; False; "cases"; New collection)
$report.compiled:=$tool._cli.compiled
HOST_Phase:="renamed executable"
If (Is Windows)
	$renamedExecutable:=File($request.executable; fk platform path)
Else
	$renamedExecutable:=File($request.executable; fk posix path)
End if
$report.renamedExecutable:=cs.MeshPreview._Process.new().execute($renamedExecutable; New collection("--version"))
If (Not($report.renamedExecutable.ok))
	$report.success:=False
End if
HOST_Phase:="feature cases"
If ($request.expectCompiled#$report.compiled)
	$report.success:=False
End if
// Deterministic, host-independent wire cases: plain --version text and the
// capabilities envelope. Render/inspect depend on staged paths and timing, so
// they are asserted structurally under the execution section instead.
$report.cases.push($tool.version())
$report.cases.push($tool.capabilities())
If (Not($report.cases[0].ok))
	$report.success:=False
End if
If (Not($report.cases[1].ok))
	$report.success:=False
End if
HOST_Phase:="render triangle"
$inputFile:=$requestFile.parent.file("triangle.stl")
$outputFile:=$requestFile.parent.file("render-tri.png")
$options:=New object("size"; 64)
$result:=$tool.render($inputFile; $outputFile; $options)
$report.renderTri:=$result
If (Not($result.ok))
	$report.success:=False
End if
HOST_Phase:="render cube"
$inputFile:=$requestFile.parent.file("cube.ply")
$outputFile:=$requestFile.parent.file("render-cube.png")
$options:=New object("size"; 128; "view"; "iso")
$result:=$tool.render($inputFile; $outputFile; $options)
$report.renderCube:=$result
If (Not($result.ok))
	$report.success:=False
End if
HOST_Phase:="inspect"
$inputFile:=$requestFile.parent.file("triangle.stl")
$result:=$tool.inspect($inputFile)
$report.inspect:=$result
If (Not($result.ok))
	$report.success:=False
End if
HOST_Phase:="invalid input"
$missingFile:=$requestFile.parent.file("missing-input.stl")
$result:=$tool.inspect($missingFile)
$report.invalid:=$result
If ($result.ok | ($result.error.code#"io_error"))
	$report.success:=False
End if
HOST_Phase:="invalid options"
$outputFile:=$requestFile.parent.file("render-invalid.png")
$result:=$tool.render($requestFile.parent.file("triangle.stl"); $outputFile; New object("size"; 0))
$report.invalidArgs:=$result
If ($result.ok | ($result.error.code#"invalid_argument"))
	$report.success:=False
End if
HOST_Phase:="missing executable"
$result:=cs.MeshPreview._CLI.new("missing-cli"; $target).execute(New collection("--version"))
$report.missing:=$result
If ($result.error.code#"missing_executable")
	$report.success:=False
End if
$result:=cs.MeshPreview.Thumbnailer.new("unsupported-arm-target").version()
$report.unsupported:=$result
If ($result.error.code#"unsupported_architecture")
	$report.success:=False
End if
HOST_Phase:="launch failure"
$result:=cs.MeshPreview._Process.new().execute($requestFile; New collection)
$report.launch:=$result
If ($result.error.code#"launch_failed")
	$report.success:=False
End if
HOST_Phase:="malformed protocol"
$result:=$tool._decode(New object("ok"; True; "stdout"; "not json"; "stderr"; ""; "exitCode"; 0))
$report.protocol:=$result
If ($result.error.code#"invalid_protocol")
	$report.success:=False
End if
$report.protocolTypes:=New collection
For each ($name; New collection("{\"schema_version\":true}"; "{\"schema_version\":false}"))
	$result:=$tool._decode(New object("ok"; True; "stdout"; $name; "stderr"; ""; "exitCode"; 0))
	$report.protocolTypes.push($result)
	If ($result.ok | ($result.error.code#"invalid_protocol"))
		$report.success:=False
	End if
End for each
$result:=$tool._decode(New object("ok"; True; "stdout"; "{\"schema_version\":null}"; "stderr"; ""; "exitCode"; 0))
$report.protocolNull:=$result
If ($result.ok | ($result.error.code#"invalid_protocol"))
	$report.success:=False
End if
$report.protocolScalars:=New collection
For each ($name; New collection("true"; "false"; "null"; "1"; "[]"))
	$result:=$tool._decode(New object("ok"; True; "stdout"; $name; "stderr"; ""; "exitCode"; 0))
	$report.protocolScalars.push($result)
	If ($result.ok | ($result.error.code#"invalid_protocol"))
		$report.success:=False
	End if
End for each
$result:=$tool._decode(New object("ok"; True; "stdout"; "{\"schema_version\":1,\"success\":false,\"error\":{\"code\":\"io_error\",\"message\":\"intentional failure\",\"path\":null,\"stage\":null}}"; "stderr"; ""; "exitCode"; 0))
$report.envelopeFailure:=$result
If (($result.ok) | ($result.error.code#"io_error") | ($result.error.path#Null))
	$report.success:=False
End if
HOST_Phase:="scalar arguments"
$report.rejectedArguments:=New collection
For each ($invalid; New collection(123; 1.25; True; False; Null; Current date; Current time; New collection; New object))
	$result:=cs.MeshPreview._Process.new().execute($requestFile; New collection($invalid))
	$report.rejectedArguments.push(New object("type"; Value type($invalid); "result"; $result))
	If ($result.ok | ($result.error.code#"invalid_argument"))
		$report.success:=False
	End if
End for each
$report.rejectedNul:=New collection
HOST_Phase:="NUL arguments"
For each ($name; New collection(Char(0)+"prefix"; "middle"+Char(0)+"suffix"; "trailing"+Char(0)))
	$result:=cs.MeshPreview._Process.new().execute($requestFile; New collection($name))
	$report.rejectedNul.push($result)
	If ($result.ok | ($result.error.code#"invalid_argument"))
		$report.success:=False
	End if
End for each
$report.missingFile:=cs.MeshPreview._Process.new().execute(Null; New collection)
$report.missingArguments:=cs.MeshPreview._Process.new().execute($requestFile; Null)
If ($report.missingFile.ok | ($report.missingFile.error.code#"invalid_argument"))
	$report.success:=False
End if
If ($report.missingArguments.ok | ($report.missingArguments.error.code#"invalid_argument"))
	$report.success:=False
End if
$report.finished:=True
ON ERR CALL(HOST_PreviousHandler; ek local)
$statusFile.setText(JSON Stringify($report))
HOST_StatusFile:=Null
HOST_Phase:=""
HOST_PreviousHandler:=""
QUIT 4D
