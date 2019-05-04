# Spinnrd
Spinnrd is a daemon that translates accelerometer data into xrandr 
orientations. It can be used to autorotate a tablet or phone's display to 
match the orientation.

## Requirements
### Building
- Cargo *(Or at least rustc, if you don't mind doing the extra work)*
### Running
- iio-sensor-proxy *(For the FSAccel backend, which is the only one 
  currently implemented)

## Basic autorotation
Start a spinnrd process as a service (you'll probably want to use 
--daemonize). Then, have your display manager run spinnr.sh (backgrounded) 
as part of its startup script. Your display should now change to match your 
device's orientation! If you want to tweak the sensitivity, spinnrd has 
a variety of command-line options for doing this.

