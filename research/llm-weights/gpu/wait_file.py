"""A job that waits (up to 30 minutes) until FILE exists in this branch, pulling every 30 s."""
import json, os, subprocess, sys, time
t0 = time.time()
while not os.path.exists(os.environ["FILE"]) and time.time() - t0 < 1800:
    subprocess.run(["git", "-C", "/home/user/chloedb", "pull", "-q", "--ff-only"], capture_output=True); time.sleep(30)
json.dump({"found": os.path.exists(os.environ["FILE"]), "secs": round(time.time() - t0)}, open(sys.argv[1], "w"))
