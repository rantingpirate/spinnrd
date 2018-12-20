Design choices for spinnrd

### A note on word usage
For the purposes of this document, I will be using "orientation"
to refer to the physical orientation of the device or representation
of such, and "rotation" to refer to the orientation of the screen.

---

# Language

Why Rust? Several reasons, really.

First and foremost among these was that I wanted to try using Rust,
but needed something of manageable scale, that wouldn't (necessarily)
be better off written in a scripting language, that didn't need a GUI.
(As of Dec. 2018, Rust still doesn't really have a good mature GUI
library or bindings, other than perhaps to qt - and what's the point
in using the wonderfully-memory-safe Rust if I'm using qt, which
requires a special set of flags to __not__ send valgrind into
screaming conniptions?)

Besides that, I wanted two things: I wanted a language that was
**fast and light** and **easy-to-write**.

## Fast and light
By fast, I mean both quick to start and low-impact to run. As for light,
well it's not like I'm doing tons of vector math-  
Okay, I'm totally doing vector math, but only a little, and not the
kind that's computationally intensive. At any rate, this thing's going
to be sitting there in the background _all the time_, so it's important
that it not use a lot of resources; to me, especially together with
fast startup, that says 'compiled language'. Python and Ruby are great,
but they're don't run (and __really__ don't start [Dec. 2018]) as fast
as compiled languages like C/C++ and Rust. Note that I'm counting Java
as being interpreted (It has a _garbage collector_ and I've died to
too many Minecraft lag spikes).

## Easy-to-Write
My other big consideration was that if writing this turned into an
exercise in pain and frustration, it'd never get done, and this is
what really finished narrowing things down. I needed a language I
was either familiar with or interested in, which among compiled
languages meant either C, C++, or Rust.

I'd been interested in Rust for a few months at that point. A
statically-typed, compiled language without having to worry about
managing memory or inheritance? Sign me up please!


# This Project
So why write this? Surely there's already software to do this, right?
Well, actually...

At the time I started this, I was using a Lenovo Flex 3 14" - essentially
the consumer version of the enterprise Yoga. It can be used in Laptop,
Stand, Easel, or Tablet configuration. The last two, however, need the
display to be able to auto-rotate for maximum usability. So I looked.

And I looked.

I found a few things, but none that worked consistently well, if at all.
And the one that worked was a GUI userspace application and didn't work
for the login manager (not, now that I think about it, that I really
tried...). So, I decided to write my own.

What you see here is the product of many man-days of effort, all to
solve one niggly little problem: **Why can't my screen just rotate
itself like it does for Windows, macOS, iOS, and Android?** It
isn't 'done' and it may never be - there'll probably always be
something I want to add - but the core functionality? It's there.
It works. And so I present to you... spinnrd.


# Why spinnrd?
This actually isn't the first iteration of the whole "use Rust to
rotate the screen to match the orientation".
That first version was called 'spinnr', and it worked. So why
rewrite it?

spinnr was written as one executable, entirely in Rust,
with bindings to xrandr. And that was cool and all, but then I
saw the problem. Since it bound to xrandr, it couldn't start without X.
Which means you'd be running a seperate instance for each session.
Not only does that mean much duplication of work, but even if I'd
gotten the iio backend working, I'm pretty sure it wouldn't have
worked in that case at all. So I rewrote it.

Now there's two parts:
-	*spinnrd* is the parts of spinnr that dealt with translating
	accelerometer readings into physical orientations. It's
	written entirely in Rust and there should be at most one
	instance per physical screen.
-	*spinnr.sh* is the (example) 'spinnr client', that watches
	for orientation changes and rotates the screen to match.
	It can have as many as one instance per physical screen
	per X server (I'm hoping to reduce this number
	in the future).


# Communication
The (first) big challenge I ran into was "how do I
communicate the orientation to several consumers?"
Specifically, how do I make the information available to multiple
simultaneous consumers, who are going to come and go?

## Write to a File
My first thought was to just write to a file. Consumers could
use inotify to know when the file changed and thus when the
screen rotation needed changing.

### Pros:
-	Uses mature, stable code
-	Easy to consume
-	Doesn't require any extra work to provide to multiple consumers
-	Can use inotify to execute code on change

### Cons:
-	You're continually writing to a file, which is problematic
	if you can't put it on a RAM-backed filesystem.
-	Not the most elegant solution
-	Does require the use of inotify to detect changes


## Events
My second thought was "Wait, couldn't I implement this as events?"
I quickly reached the conclusion that events are for drivers, Clu.


## Dbus
So what about Dbus? Isn't it meant for IPC?

Well yes, it is; however, from what I gathered from my
(admittedly somewhat cursory) look at it, it requires the sender
know the recipient ahead of time. That, and I *really* don't want to
have to implement responding to queries - this is spinnrd,
not spinnrsrv.


## Use a Unix Domain Socket
Spinnrd is intended for Linux, right? So how about a socket?

This is a lovely idea, but while it's true that multiple consumers can
connect to a single socket, it'd be more accurate to say that
multiple consumers can use the same socket to establish 1-1
connections with the provider, which has to listen for those
connections, establish them, and write to each of them individually
after checking that they're still open (see previous comment
about this being spinnrd).

That, and Rust doesn't have any mature UDS libraries. [Dec. 2018]


## Use a (Network) Socket
All of the problems of a domain socket and more besides.  
Gee, lemme think...


## Write to a Pipe
Okay, so how about a pipe?

While a pipe would alleviate the problem of writing to disk,
they don't support multiple consumers. There'd need to be a pipe for
each consumer, at which point it would be like using a socket,
only clunkier and worse.


## Direct Memory Access
No. Just... **_no_**.


## Decision:
In the end, it seems that writing to a file is my best option,
at least until someone comes up with a better way of intra-system
info broadcasting for Linux. Or points me towards one I missed.


# Filename Formatting
*%e*: The current local epoch time (in seconds.nanoseconds)
*%E*: The current UTC epoch time (in seconds.nanoseconds)
*%f*: The spinnr directory
*%d*: The current local date and time, in basic ISO 8601 format (YYYYmmddTHHMMSS.NN±hhmm)
*%u*: The current UTC date and time, in basic ISO 8601 format (YYYYmmddTHHMMSS.NN±hhmm)

# Opening the file to write it
This was something of a hard decision for me. It requires more
work to do, but it makes inotifywait _much_ more feasible to use,
so it's just the way I'm going to go.
