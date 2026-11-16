"""`python -m chloedb_server ARGS` runs `chloedb-server ARGS`, for an
environment whose programs are not on the PATH."""

import os
import sys

from chloedb_server import find_bin


def main() -> None:
    exe = find_bin()
    # The server takes this process's place, so signals (Ctrl-C, a
    # container's stop) reach it.
    os.execv(exe, [exe, *sys.argv[1:]])


if __name__ == "__main__":
    main()
