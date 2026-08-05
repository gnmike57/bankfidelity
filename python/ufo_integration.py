import subprocess
import os
import json
from typing import Dict, Any

def run_ufo_task(task_description: str, app_name: str = "chrome") -> Dict[str, Any]:
    """
    Executes a Microsoft UFO (UI-Focused Agent) task.
    Requires UFO to be installed and accessible in the environment.
    """
    try:
        # We assume UFO is accessible via python -m ufo or similar in the system
        # UFO expects tasks to be provided to its agent.
        # This is a wrapper that dispatches to the UFO CLI.
        
        # Example UFO invocation: python -m ufo --task "download statement" --app "chrome"
        ufo_script = os.path.join(os.path.expanduser("~"), "UFO", "ufo", "ufo.py")
        
        if not os.path.exists(ufo_script):
            return {
                "status": "error",
                "message": f"UFO framework not found at {ufo_script}. Please clone https://github.com/microsoft/UFO."
            }
            
        cmd = [
            "python", ufo_script,
            "--task", task_description,
            "--app", app_name
        ]
        
        result = subprocess.run(cmd, capture_output=True, text=True, check=True)
        
        return {
            "status": "success",
            "output": result.stdout
        }
        
    except subprocess.CalledProcessError as e:
        return {
            "status": "error",
            "message": f"UFO task failed: {e.stderr}"
        }
    except Exception as e:
        return {
            "status": "error",
            "message": f"Failed to invoke UFO: {str(e)}"
        }
