#!/bin/sh

curl http://downloads.majestic.com/majestic_million.csv | grep -v ,Domain | awk -F ',' '{print $3}' >topsites.txt
