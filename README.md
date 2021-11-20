# Spinnrd
Spinnrd is a pair of daemons to automatically rotate a device's screen to
match its orientation. It has two parts: the `spinnrd` system daemon that
translates accelerometer data into xrandr orientations, and the `spinnr.sh`
daemon that makes the xrandr and xinput calls. `spinnr.sh` needs to be
called for each X instance, but only one `spinnrd` is needed per physical
display to rotate.

## Installation

### Arch Linux

AUR: [spinnrd-git](https://aur.archlinux.org/packages/spinnrd-git/)

With `yay`:
```bash
yay -S spinnrd-git
```

### Notes

Ensure to start/enable the `spinnrd.service` systemd service, and add the
`/usr/share/spinnr/spinnr.sh` script to your desktop's auto-start applications.

## Requirements
### Building
- [Rust](rust); more specifically, [Cargo](cargo) *(Cargo technically isn't
  necessary as long as you have the Rust compiler, but it'll save you tons
  of work)*
### Running
- iio-sensor-proxy *(For the FSAccel backend, which is the only one
  currently implemented)*

## Basic usage
Start a `spinnrd` process, probably as a service (you'll probably want to
use --daemonize). Then, have your display manager run `spinnr.sh`
(backgrounded) as part of its startup script. Your display should now
rotate to match your device's orientation! If you want to tweak the
sensitivity, `spinnrd` has a variety of command-line options for doing
this.

# About This Project

### Why did I write this?
So why write this? Surely there's already software to do this, right?
Well, actually...

At the time I started this, I was using a Lenovo Flex 3 14" - essentially
the consumer version of the enterprise Yoga. It can be used in Laptop,
Stand, Easel, or Tablet configuration. However, for the last two to be
usable the screen needs to rotate, and for Tablet in particular it needs to
do so automatically. So I looked.

And I looked.

I found a few things, but none that worked consistently well, if at all.
And the one that worked was a GUI userspace application and didn't work for
the login manager (not, now that I think about it, that I really tried...).
So, I decided to write my own.

What you see here is the product of many man-days of effort, all to solve
one niggly little problem: **Why can't my screen just rotate itself like it
does for Windows, macOS, iOS, and Android?** It isn't 'done' and it may
never be - there'll probably always be something I want to add - but the
core functionality? It's there.  It works. And so I present to you...
spinnrd.

### Where'd the name come from?
This actually isn't the first iteration of the whole "use Rust to rotate
the screen to match the orientation".  That first version was called
'spinnr', and it worked. So why rewrite it?

spinnr was written as one executable, entirely in Rust, with bindings to
xrandr. And that was cool and all, but then I saw the problem. Since it
bound to xrandr, it couldn't start without X.  Which means you'd be running
a seperate instance for each session.  Not only does that mean much
duplication of work, but even if I'd gotten the iio backend working, I'm
pretty sure it wouldn't have worked in that case at all. So I rewrote it.

Now there's two parts:
-	*spinnrd* is the parts of spinnr that dealt with translating
	accelerometer readings into physical orientations. It's written
entirely in Rust and there should be at most one instance per physical
screen.
-	*spinnr.sh* is the (example) 'spinnr client', that watches for
	orientation changes and rotates the screen to match.  It can have as
many as one instance per physical screen per X server (I'm hoping to reduce
this number in the future).

# Contributing
I'm always happy to recieve contributions - that's one of the major
benefits of open-source software, you're not alone. If you want an idea on
where to get started, check for open issues and take a look at
`roadmap.md`. I'll try to get a Trello up and running as well.

[rust]: https://rust-lang.org
[cargo]: https://doc.rust-lang.org/cargo/
