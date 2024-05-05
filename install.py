#! /usr/bin/env python

import os
import shutil
import sys
import argparse
import errno

DEFAULT_INSTALL_DIR = "/usr/local/bin"
DATA_DIR = "/usr/share/flowd"

FALLBACK_CONFIG_DIR = os.path.join(DATA_DIR, "config")
DB_MIGRATIONS_DIR = os.path.join(DATA_DIR, "migrations")

SOURCE_CONFIG_PATH = "src/resources/config/config.toml"
SOURCE_MIGRATIONS_DIR = "src/resources/db/migrations"
SOURCE_DEBUG_TARGET_DIR = "target/debug"
SOURCE_RELEASE_TARGET_DIR = "target/release"

TARGET_FILENAME = "flowd"

dirs_to_clean = []
files_to_clean = []

def print_err(*args, **kwargs):
  print(*args, file=sys.stderr, **kwargs)

def install(args):
  if not args.no_target:
    target_dir = (SOURCE_DEBUG_TARGET_DIR if args.debug else SOURCE_RELEASE_TARGET_DIR)
    target_path = os.path.join(target_dir, TARGET_FILENAME)

    # Check if binary is compiled
    if not os.path.exists(target_path):
      print_err("Compile binary before installing")
      print("Exiting...")
      sys.exit(1)

    print("Installing target...")
    target_install_path = os.path.join(args.install_path, TARGET_FILENAME)
    if os.path.exists(target_install_path):
      os.remove(target_install_path)
    shutil.move(target_path, args.install_path)
    files_to_clean.append(target_install_path)

  # Create data dir
  if not os.path.exists(DATA_DIR):
    os.mkdir(DATA_DIR)
  dirs_to_clean.append(DATA_DIR)

  # Create fallback config dir & copy config
  print("Copying default config...")
  if not os.path.exists(FALLBACK_CONFIG_DIR):
    os.mkdir(FALLBACK_CONFIG_DIR)
  shutil.copy(SOURCE_CONFIG_PATH, FALLBACK_CONFIG_DIR)

  # Create migrations dir & copy migrations
  print("Copying database migrations...")
  if os.path.exists(DB_MIGRATIONS_DIR):
    shutil.rmtree(DB_MIGRATIONS_DIR)
  os.mkdir(DB_MIGRATIONS_DIR)
  for file in os.listdir(SOURCE_MIGRATIONS_DIR):
    file_path = os.path.join(SOURCE_MIGRATIONS_DIR, file)
    shutil.copy(file_path, DB_MIGRATIONS_DIR)

def cleanup():
  print("Cleaning up...")
  for file in files_to_clean:
    os.remove(file)
  for dir in dirs_to_clean:
    shutil.rmtree(dir)

parser = argparse.ArgumentParser("install.sh", description="Installer for flowd")
parser.add_argument("-i", "--install-path", help="Binary install destination path", default=DEFAULT_INSTALL_DIR)
parser.add_argument("-d", "--debug", action="store_true", help="Installs the debug binary instead of release binary")
parser.add_argument("-n", "--no-target", action="store_true", help="Copies data files only without target")

args = parser.parse_args()

try:
  install(args)
except Exception as e:
  print_err(e)
  cleanup()