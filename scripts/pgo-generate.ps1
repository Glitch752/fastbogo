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
    Write-Host "llvm-profdata is required for PGO."
    Write-Host "Install it with: rustup component add llvm-tools-preview"
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

$ProfileDir = if ($env:PROFILE_DIR) { $env:PROFILE_DIR } else { Join-Path $RootDir "target\pgo-data" }
$BenchCount = if ($env:BENCH_COUNT) { $env:BENCH_COUNT } else { "1000000000" }
$BenchThreads = if ($env:BENCH_THREADS) {
    $env:BENCH_THREADS
} else {
    [Environment]::ProcessorCount
}
$BenchWarmup = if ($env:BENCH_WARMUP) { $env:BENCH_WARMUP } else { "1" }
$BenchRounds = if ($env:BENCH_ROUNDS) { $env:BENCH_ROUNDS } else { "3" }

if (Test-Path $ProfileDir) {
    Remove-Item $ProfileDir -Recurse -Force
}
New-Item -ItemType Directory -Path $ProfileDir | Out-Null

$env:RUSTFLAGS = "-C target-cpu=native -C profile-generate=$ProfileDir"

cargo build --release

& ".\target\release\fastbogo.exe" `
    --benchmark `
    --count $BenchCount `
    --threads $BenchThreads `
    --benchmark-warmup-rounds $BenchWarmup `
    --benchmark-rounds $BenchRounds

$profraw = Get-ChildItem "$ProfileDir\*.profraw"

& $llvmProfdata merge `
    -output="$ProfileDir\merged.profdata" `
    $profraw.FullName

Write-Host "generated $ProfileDir\merged.profdata"