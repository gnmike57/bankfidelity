# start_llama_server.ps1
# Deploys llama-server.exe for Local LLM integration with UFO and BankFidelity

$llamaPath = "llama-server.exe"
$modelPath = "C:\ufo\models\qwen2.5-coder-7b-instruct-q4_k_m.gguf"

if (-Not (Test-Path $modelPath)) {
    Write-Host "ERROR: Model not found at $modelPath"
    Write-Host "Please download the Qwen 2.5 Coder 7B GGUF and place it in the models directory."
    exit 1
}

Write-Host "Starting llama-server.exe with Vulkan offloading (-ngl 99) and 16k context..."
& $llamaPath -m $modelPath -c 16384 -ngl 99 --port 11434
