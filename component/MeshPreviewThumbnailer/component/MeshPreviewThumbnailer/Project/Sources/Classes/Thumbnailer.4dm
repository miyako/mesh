property _cli : cs._CLI

Class constructor($target : Text)
	This._cli:=cs._CLI.new("mesh-thumb"; $target)

// MeshPreview.Thumbnailer renders STL/PLY geometry with the bundled mesh-thumb
// CLI (spec 22). All methods keep CLI stdout untouched; _decode normalizes the
// mesh-thumb {schema_version, success, result|error} envelope into wrapper
// results, preserving structured errors carried on stdout with nonzero exits.

Function capabilities()->$result : Object
	$result:=This._decode(This._cli.execute(New collection("capabilities"; "--json")))

Function inspect($InputFile : 4D.File)->$result : Object
	If (Not(OB Instance of($InputFile; 4D.File)))
		return New object("ok"; False; "error"; New object("code"; "invalid_argument"; "message"; "Input geometry File is required"))
	End if
	$result:=This._decode(This._cli.execute(New collection("inspect"; $InputFile; "--json")))

Function version()->$result : Object
	var $stdout; $version : Text
	var $start; $length : Integer
	var $groups : Collection
	$result:=This._cli.execute(New collection("--version"))
	If (Not($result.ok))
		return $result
	End if
	$stdout:=Trim($result.stdout)
	$groups:=New collection
	If (Match regex("^mesh-thumb ([0-9]+\\.[0-9]+\\.[0-9]+)$"; $stdout; 1; $start; $length; $groups))
		$version:=$groups[0]
		$result.value:=New object("name"; "mesh-thumb"; "version"; $version)
	Else
		$result.ok:=False
		$result.error:=New object("code"; "invalid_protocol"; "message"; "mesh-thumb --version must match mesh-thumb <semver>, got: " + $stdout)
	End if

Function render($InputFile : 4D.File; $OutputFile : 4D.File; $options : Object)->$result : Object
	// render <FILE> -o <PNG> [--size N] [--view iso|top|front|side]
	//        [--background #RRGGBB|transparent] [--quality N] --json
	var $arguments : Collection
	var $size; $quality : Integer
	var $view; $background : Text
	If (Not(OB Instance of($InputFile; 4D.File)))
		return New object("ok"; False; "error"; New object("code"; "invalid_argument"; "message"; "Input geometry File is required"))
	End if
	If (Not(OB Instance of($OutputFile; 4D.File)))
		return New object("ok"; False; "error"; New object("code"; "invalid_argument"; "message"; "Output PNG File is required"))
	End if
	$arguments:=New collection("render"; $InputFile; "-o"; $OutputFile)
	If ($options#Null)
		$size:=0
		If ((Value type($options.size)#Is undefined) & (Value type($options.size)#Is null))
			If (((Value type($options.size)=Is integer) | (Value type($options.size)=Is longint)) & ($options.size>0))
				$size:=$options.size
				$arguments.push("--size"; String($size))
			Else
				return New object("ok"; False; "error"; New object("code"; "invalid_argument"; "message"; "options.size must be a positive integer"))
			End if
		End if
		If ((Value type($options.view)#Is undefined) & (Value type($options.view)#Is null))
			$view:=$options.view
			If (Match regex("^(iso|top|front|side)$"; $view))
				$arguments.push("--view"; $view)
			Else
				return New object("ok"; False; "error"; New object("code"; "invalid_argument"; "message"; "options.view must be iso, top, front or side"))
			End if
		End if
		If ((Value type($options.background)#Is undefined) & (Value type($options.background)#Is null))
			$background:=$options.background
			If (Match regex("^(#[0-9a-fA-F]{6}|transparent)$"; $background))
				$arguments.push("--background"; $background)
			Else
				return New object("ok"; False; "error"; New object("code"; "invalid_argument"; "message"; "options.background must be #RRGGBB or transparent"))
			End if
		End if
		If ((Value type($options.quality)#Is undefined) & (Value type($options.quality)#Is null))
			If (((Value type($options.quality)=Is integer) | (Value type($options.quality)=Is longint)) & (($options.quality>=1) & ($options.quality<=8)))
				$quality:=$options.quality
				$arguments.push("--quality"; String($quality))
			Else
				return New object("ok"; False; "error"; New object("code"; "invalid_argument"; "message"; "options.quality must be an integer from 1 to 8"))
			End if
		End if
	End if
	$arguments.push("--json")
	$result:=This._decode(This._cli.execute($arguments))
	If ($result.ok)
		// Validate the returned PNG before exposing it (spec 22.3).
		If (Not(This._pngValid($OutputFile)))
			$result.ok:=False
			$result.error:=New object("code"; "invalid_png"; "message"; "rendered output is not a valid PNG"; "path"; $OutputFile.path)
			Return $result
		End if
		$result.pngValid:=True
		$result.pngSize:=$OutputFile.size
	End if

Function _pngValid($file : 4D.File)->$valid : Boolean
	var $handle : 4D.FileHandle
	var $bytes : Blob
	$valid:=False
	If ($file.exists)
		$handle:=$file.open()
		If ($handle#Null)
			$bytes:=$handle.read(8)
			$handle.close()
			If (Length($bytes)=8)
				If (BLOB to longint($bytes; big endian; 0)=-1995860921) // 0x89504E47 PNG magic
					$valid:=(BLOB to longint($bytes; big endian; 4)=218765834) // 0x0D0A1A0A
				End if
			End if
		End if
	End if

Function _decode($result : Object)->$decoded : Object
	var $payload : Variant
	var $previous : Text
	var $parseErrors : Collection
	var CLI_LastErrors : Collection
	If ($result.stdout="")
		If (Not($result.ok))
			// launch/process failures carry no envelope
			return $result
		End if
	End if
	$previous:=Method called on error(ek local)
	CLI_LastErrors:=Null
	ON ERR CALL("_CaptureError"; ek local)
	$payload:=JSON Parse($result.stdout)
	ON ERR CALL($previous; ek local)
	$parseErrors:=CLI_LastErrors
	CLI_LastErrors:=Null
	If (($parseErrors#Null) | (Value type($payload)#Is object))
		$result.ok:=False
		$result.error:=New object("code"; "invalid_protocol"; "message"; "CLI response must be a JSON object"; "details"; $parseErrors)
		return $result
	End if
	If ($payload.schema_version#1)
		$result.ok:=False
		$result.error:=New object("code"; "invalid_protocol"; "message"; "Unsupported CLI protocol version")
		return $result
	End if
	If ($payload.success#True)
		// Structured failure envelope on stdout; exit codes are advisory.
		$result.ok:=False
		If ((Value type($payload.error)=Is object) & (Value type($payload.error.code)=Is text))
			$result.error:=New object("code"; $payload.error.code; "message"; $payload.error.message; "path"; $payload.error.path; "stage"; $payload.error.stage)
		Else
			$result.error:=New object("code"; "cli_error"; "message"; "mesh-thumb reported a failure")
		End if
		return $result
	End if
	$result.value:=$payload.result
	$decoded:=$result
