#! /usr/bin/env python

import os
import shutil
import sys
import argparse
import errno

DEFAULT_INSTALL_PATH = "/usr/local/bin"

def print_err(*args, **kwargs):
  print(*args, file=sys.stderr, **kwargs)

def install():
  # Check if binary is compiled
  if not os.path.exists("target/release/flowd"):
    print_err("Compile binary before installing")
    print("Exiting...")
    sys.exit(1)

  print("Compilation successful. Installing target...")
  shutil.move("target/release/flowd", args.install_path)

  # Check if flow directory exists in /etc
  if not os.path.exists("/etc/flow"):
    os.mkdir("/etc/flow")

  print("Copying default configuration to /etc")

  # Install default configuration to /etc/flow
  shutil.copy("src/resources/config/config.toml", "/etc/flow/config.toml")


parser = argparse.ArgumentParser("install.sh", description="Installer for flowd")
parser.add_argument("-i", "--install-path", help="Binary install destination path", default=DEFAULT_INSTALL_PATH)

args = parser.parse_args()

try:
  install()
except OSError as e:
  if e.errno == errno.EPERM:
    print_err("Permissions error. Make sure you execute this installer with root privileges")
  else:
    raise e
