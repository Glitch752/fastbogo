$ErrorActionPreference = "Stop"

$RootDir = Split-Path $PSScriptRoot -Parent
Set-Location $RootDir

function Find-LlvmProfdata {
    if ($env:LLVM_PROFDATA) {
        return $env:LLVM_PROFDATA
    }

    $sysroot = & rustc --print sysroot
    $rust_host = (& rustc -vV | Select-String '^host:' | ForEach-Object {
        ($_ -split '\s+')[1]
    })

    $tool = Join-Path $sysroot "lib\rustlib\$rust_host\bin\llvm-profdata.exe"
    if (Test-Path $tool) {
        return $tool
    }

    $cmd = Get-Command llvm-profdata.exe -ErrorAction SilentlyContinue
    if ($cmd) {
        return $cmd.Source
    }

    return $null
}

function Get-RustLlvmMajor {
    (& rustc -vV | Select-String '^LLVM version:' | ForEach-Object {
        (($_ -split '\s+')[2] -split '\.')[0]
    })
}

function Get-ToolLlvmMajor($tool) {
    (& $tool --version 2>$null | Select-String 'LLVM version' | ForEach-Object {
        (($_ -split '\s+')[3] -split '\.')[0]
    })
}

$llvmProfdata = Find-LlvmProfdata
if (-not $llvmProfdata) {
    Write-Host "llvm-profdata is required for PGO validation."
    Write-Host "Install the matching Rust toolchain component with:"
    Write-Host "    rustup component add llvm-tools-preview"
    exit 1
}

$rustLlvm = Get-RustLlvmMajor
$toolLlvm = Get-ToolLlvmMajor $llvmProfdata

if (-not $toolLlvm -or $toolLlvm -ne $rustLlvm) {
    $toolLlvm = if ($toolLlvm) { $toolLlvm } else { "unknown" }
    Write-Host "llvm-profdata version mismatch: rustc uses LLVM $rustLlvm, but $llvmProfdata is LLVM $($toolLlvm)"
    Write-Host "Install the matching Rust toolchain component with:"
    Write-Host "    rustup component add llvm-tools-preview"
    exit 1
}

$ProfileDir = if ($env:PROFILE_DIR) {
    $env:PROFILE_DIR
} else {
    Join-Path $RootDir "target\pgo-data"
}

$Profdata = Join-Path $ProfileDir "merged.profdata"

if (-not (Test-Path $Profdata)) {
    Write-Host "Missing $Profdata; run scripts\pgo-generate.ps1 first."
    exit 1
}

# Validate the profile
& $llvmProfdata show $Profdata *> $null

$env:RUSTFLAGS = "-C target-cpu=native -C profile-use=$Profdata -C llvm-args=-pgo-warn-missing-function"

cargo build --release

Write-Host "Built release binary with PGO profile $Profdata"