$env:PYTHON_EXECUTABLE='C:\Users\zbook\Downloads\python-3.15.0rc1-embed-amd64\python.exe'
& $env:PYTHON_EXECUTABLE -c "import sys; sys.path.append('python'); from worker import WorkerRuntime; w = WorkerRuntime(); print(w.handshake())"
