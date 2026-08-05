$env:PYTHON_EXECUTABLE='C:\Users\zbook\Downloads\python-3.15.0rc1-embed-amd64\python.exe'
Invoke-WebRequest -Uri https://bootstrap.pypa.io/get-pip.py -OutFile get-pip.py
& $env:PYTHON_EXECUTABLE get-pip.py
& $env:PYTHON_EXECUTABLE -m pip install PyMuPDF
