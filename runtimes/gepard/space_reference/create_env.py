import os
import subprocess
import sys

def setup_dependencies():
    os.environ["OMP_NUM_THREADS"] = "4"
    try:
        if os.path.exists('/tmp/deps_installed'):
            return
            
        # Re-pin transformers on top of whatever NeMo downgraded at build time
        # (the Gepard checkpoint needs the Qwen3.5 backbone from 5.x). This
        # upgrades huggingface-hub to 1.x — gradio 5.49 accepts <2.0, so the
        # UI keeps working. numpy is capped so the force-reinstall does not
        # bump it to 2.x under the NeMo stack built against 1.26.
        print("Re-pinning transformers==5.3.0 ...")
        subprocess.check_call([
            sys.executable, "-m", "pip", "install", "--force-reinstall", "--no-cache-dir",
            "transformers==5.3.0", "numpy<2.0"
        ])
        
        with open('/tmp/deps_installed', 'w') as f:
            f.write('done')
            
    except Exception as e:
        print(f"Dependencies setup error: {e}")