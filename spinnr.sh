#!/bin/sh

#FIXME: add output autodetection
#FIXME: add args for spinfile and output name
#TODO: interactive output choice
#TODO: additional xrandr args(?) (e.g. quiet)
while inotifywait -qqe close_write /tmp/spinnrd.spin; do
  xrandr --output eDP1 --rotate $(cat /tmp/spinnrd.spin);
done
