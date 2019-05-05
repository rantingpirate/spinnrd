#!/bin/bash

function usage {
	cat - <<EOF
This is spinnr.sh, the client daemon of the linux screen autorotation daemons.
Usage: $0 spinfile [-o output | -t touchscreen]...

    -h, --help      Display this help and exit.
    -t TOUCHSCREEN,
      --touchscreen=TOUCHSCREEN     Add the name of a touchscreen to rotate.
    -o OUTPUT,
      --output=OUTPUT               Add the name of an output to rotate.
    -q, --quiet     Don't output text.
EOF
}

selfdir=$(dirname "$(readlink $0 -m)")
if [[ "$selfdir" == "." ]]; then
	selfdir=$(pwd)
fi
quiet=0
outputs=( )
touchscreennames=( )

shortopts="hqt:o:"
longopts="help,quiet,touchscreen:,output:"
getopterrname="$0->getopt.sh"

ARGS=$(getopt -o $shortopts -l "$longopts" -n "$getopterrname" -- "$@")
if (($?)); then exit 1; fi
eval set -- "$ARGS"

while (($#)); do
	case "$1" in
		-h|--help)
			usage
			exit 0
			;;
		-o|--output)
			outputs+=( "$2" )
			shift 2
			;;
		-q|--quiet)
			quiet=1
			shift
			;;
		-t|--touchscreen)
			touchscreennames+=( "$2" )
			shift 2
			;;
		--)
			shift
			break
			;;
		*)
			echo "$1 is not a recognized option! ($0)" >&2
			usage
			exit 1
			;;
	esac
done

spinfile="$1"
if ! [[ "$spinfile" ]]; then
	echo This script needs a spinfile to function! >&2
	exit 2
elif ! [[ -f "$spinfile" ]]; then
	echo "'$spinfile' doesn't exist!" >&2
	exit 2
fi

if ! (( ${#outputs} )); then
	outputs=( $(xrandr -q | grep -Pom1 '\w+(?= connected)') )
	echo "Using autodetected output $outputs"
fi

declare -A rotmap
rotmap[normal]=" \
	 1  0  0 \
 	 0  1  0 \
 	 0  0  1 "
rotmap[left]=" \
	 0 -1  1 \
 	 1  0  0 \
 	 0  0  1 "
rotmap[inverted]=" \
	-1  0  1 \
 	 0 -1  1 \
   	 0  0  1 "
rotmap[right]=" \
	 0  1  0 \
   	-1  0  1 \
     0  0  1 "

# Convert touchscreen names to touchscreen ids
if (( ${#touchscreennames[@]} )); then
	# inputs="$(xinput --list)"
	tsc=( )
	for tsname in "${touchscreennames[@]}"; do
		tsc+=( -e )
		tsc+=( "$tsname" )
		# touchscreens+=( $(echo "$inputs" | grep -i "$tsname" | grep -Po "(id=)\d+") )
	done
	touchscreens=( $(xinput --list | grep -i "${tsc[@]}" | grep -Po '(?<=id=)\d+') )
fi

#TODO: proper option parsing
#TODO: interactive output choice
#TODO: additional xrandr args(?) (e.g. quiet)
while inotifywait -qqe close_write "$spinfile"; do
	rotation=$(cat "$spinfile")
	for output in "${outputs[@]}"; do
		xrandr --output "$output" --rotate $rotation;
	done
	if (( ${#touchscreens} )); then
		for touchscreen in "${touchscreens[@]}"; do
			xinput set-prop "$touchscreen" 'Coordinate Transformation Matrix' ${rotmap[$rotation]}
		done
	fi
done
