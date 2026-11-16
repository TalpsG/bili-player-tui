# Windows x64 MSVC helper. Supply a matching libmpv DLL and MSVC import library.
# Cargo arguments are forwarded; omitted arguments mean `cargo build`.
# Example: .\build.ps1 -MpvRoot 'C:\Libraries\libmpv' run --release
# Read raw script arguments; quote Cargo's separator as '--' in PowerShell.
$MpvRoot = if ($env:LIBMPV_ROOT) { $env:LIBMPV_ROOT } else { $env:MPV_DIR }
$CargoArgs = @()
$afterSeparator = $false
for ($i = 0; $i -lt $args.Count; $i++) {
    if (-not $afterSeparator -and $args[$i] -eq '-MpvRoot') {
        $i++
        if ($i -ge $args.Count) {
            Write-Error '-MpvRoot requires a directory path.'
            exit 1
        }
        $MpvRoot = $args[$i]
    } else {
        if ($args[$i] -eq '--') { $afterSeparator = $true }
        $CargoArgs += [string]$args[$i]
    }
}

$ErrorActionPreference = 'Stop'
$exitCode = 1
$oldPath = $env:PATH
$oldEncodedFlags = $env:CARGO_ENCODED_RUSTFLAGS
$hadEncodedFlags = Test-Path Env:CARGO_ENCODED_RUSTFLAGS
try {
    if ($env:OS -ne 'Windows_NT') {
        throw 'build.ps1 requires Windows with the x86_64-pc-windows-msvc Rust toolchain.'
    }
    if (-not $MpvRoot) {
        throw 'Set LIBMPV_ROOT (or MPV_DIR) or pass -MpvRoot pointing to a libmpv development directory.'
    }
    $root = (Resolve-Path -LiteralPath $MpvRoot).ProviderPath
    $libDir = Join-Path $root 'lib'
    $binDir = Join-Path $root 'bin'
    if (-not (Test-Path -LiteralPath (Join-Path $libDir 'mpv.lib') -PathType Leaf)) {
        throw "Missing $libDir\mpv.lib. Obtain an x64 MSVC import library matching your libmpv DLL; mpv.exe alone is insufficient."
    }
    $dlls = @(Get-ChildItem -LiteralPath $binDir -Filter '*.dll' -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^(lib)?mpv(-\d+)?\.dll$' })
    if ($dlls.Count -eq 0) {
        throw "Missing libmpv DLL in $binDir (for example mpv-2.dll or libmpv-2.dll). Keep its original name and put dependency DLLs in bin as well."
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw 'cargo was not found. Install Rust stable with the x86_64-pc-windows-msvc toolchain and Visual Studio C++ Build Tools / Windows SDK.'
    }
    # Cargo gives encoded flags precedence. Its ordinary RUSTFLAGS syntax splits
    # on whitespace; encoded arguments keep the native path intact, even with spaces.
    $separator = [string][char]0x1f
    if ($hadEncodedFlags) {
        $flags = $oldEncodedFlags
    } else {
        $flags = (@($env:RUSTFLAGS -split '\s+' | Where-Object { $_ }) -join $separator)
    }
    $nativeFlag = "-Lnative=$libDir"
    $env:CARGO_ENCODED_RUSTFLAGS = if ($flags) { $flags + $separator + $nativeFlag } else { $nativeFlag }
    $env:PATH = $binDir + [IO.Path]::PathSeparator + $oldPath
    if (-not $CargoArgs -or $CargoArgs.Count -eq 0) {
        $CargoArgs = @('build')
    } elseif ($CargoArgs[0].StartsWith('-')) {
        $CargoArgs = @('build') + $CargoArgs
    }
    # Place the target before `--`, which separates application/test arguments.
    $cargoOptions = @($CargoArgs | Select-Object -Skip 1)
    foreach ($arg in $cargoOptions) {
        if ($arg -eq '--') { break }
        if ($arg -eq '--target' -or $arg.StartsWith('--target=')) {
            throw 'This helper selects x86_64-pc-windows-msvc; do not pass --target.'
        }
    }
    $CargoArgs = @($CargoArgs[0], '--target', 'x86_64-pc-windows-msvc') + $cargoOptions
    & cargo @CargoArgs
    $exitCode = $LASTEXITCODE
} catch {
    Write-Error -Message $_ -ErrorAction Continue
} finally {
    $env:PATH = $oldPath
    if ($hadEncodedFlags) {
        $env:CARGO_ENCODED_RUSTFLAGS = $oldEncodedFlags
    } else {
        Remove-Item Env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
    }
}
exit $exitCode
