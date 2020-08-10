#!/bin/sh

cargo tree -e normal,build --no-dedupe --prefix none | sort | uniq -c | sort -nr
