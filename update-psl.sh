#!/bin/sh

set -euxo pipefail

cd src/psl

curl -s -O https://publicsuffix.org/list/public_suffix_list.dat
gzip -f -9 public_suffix_list.dat

date -u +"%Y-%m-%dT%H:%M:%SZ" >date.txt
