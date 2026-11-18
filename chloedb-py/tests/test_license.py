"""The wheel's licence (LENA-202 of the persona study): chloedb 0.3.7's
wheels carried no LICENSE. The package's LICENSE is a copy of the
repository's, declared in pyproject.toml, so that maturin puts it in every
wheel's dist-info/licenses (py-release.yml checks each wheel it builds)."""
import os
import tomllib

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")


def read(*path):
    with open(os.path.join(ROOT, *path), "rb") as f:
        return f.read()


def test_the_package_licence_is_the_repository_licence():
    assert read("chloedb-py", "LICENSE") == read("LICENSE"), "copy the repository's LICENSE to chloedb-py/LICENSE"


def test_the_wheel_declares_its_licence_file():
    project = tomllib.loads(read("chloedb-py", "pyproject.toml").decode())["project"]
    assert project["license"] == "Apache-2.0"
    assert project["license-files"] == ["LICENSE"]
