#!/bin/sh

function usage {
	echo "Usage: $0 spinfile [output]"
}

if [[ "--help" = "$1" ]]; then
	usage
	exit
fi

spinfile=$1
if ! [[ "$spinfile" ]]; then
	echo $(usage) >&2
	echo "Need a spinfile!" >&2
	exit 1
fi
if ! [[ -f "$spinfile" ]]; then
	echo "'$spinfile' doesn't exist!" >&2
	exit 2
fi

output=$2

if ! [[ "$output" ]]; then
	output=$(xrandr -q | grep -Pom1 '\w+(?= connected)')
	echo "Using autodetected output $output"
fi

#TODO: proper option parsing
#TODO: interactive output choice
#TODO: additional xrandr args(?) (e.g. quiet)
while inotifywait -qqe close_write "$spinfile"; do
  xrandr --output "$output" --rotate $(cat "$spinfile");
  #FIXME: rotate touchscreen sensing to match!
done
