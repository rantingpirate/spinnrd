Design choices for spinnrd

### A note on word usage
For the purposes of this document, I will be using "orientation" to refer 
to the physical orientation of the device or representation of such, and 
"rotation" to refer to the orientation of the screen.

For those who don't know it, **feep** is a neologism for feature creep 
("creeping feature" -> "feeping creature" -> "feep"). I use it to describe 
cool ideas that are absolutely not part of the core functionality.

<!---
  ### A note about this document
  Technically, this is intended to store my design decisions. Because of the 
  way I work, it has a tendancy to also end up as a memo pad for me to jot 
  notes for later. I try to remove those when I'm done
--->

---

# Design Goals
A list of the things that core goals of this software
- Reliable (rotates w/in 2sec of being held vertically)
- Rotates login screen (not just a feature of desktop environment)
- Efficient (fast language, only one process querying accelerometer)
- Only rotates as appropriate (if orientation changes 180°, rotation 
  changes 1x180°, not 2x90°)
- Flexible (choice of frontends, backends, logging)

# Language

Why Rust? Several reasons, really.

First and foremost among these was that I wanted to try using Rust, but 
needed something of manageable scale, that wouldn't (necessarily) be better 
off written in a scripting language, that didn't need a GUI.  (As of Dec. 
2018, Rust still doesn't really have a good mature GUI library or bindings, 
other than perhaps to qt - and what's the point in using the 
wonderfully-memory-safe Rust if I'm using qt, which requires a special set 
of flags to __not__ send valgrind into screaming conniptions?)

Besides that, I wanted two things: I wanted a language that was **fast and 
light** and **easy-to-write**.

## Fast and light
By fast, I mean both quick to start and low-impact to run. As for light, 
well it's not like I'm doing tons of vector math-  Okay, I'm totally doing 
vector math, but only a little, and not the kind that's computationally 
intensive. At any rate, this thing's going to be sitting there in the 
background _all the time_, so it's important that it not use a lot of 
resources; to me, especially together with fast startup, that says 
'compiled language'. Python and Ruby are great, but they're don't run (and 
__really__ don't start [Dec. 2018]) as fast as compiled languages like 
C/C++ and Rust. Note that I'm counting Java as being interpreted (It has 
a _garbage collector_ and I've died to too many Minecraft lag spikes).

## Easy-to-Write
My other big consideration was that if writing this turned into an exercise 
in pain and frustration, it'd never get done, and this is what really 
finished narrowing things down. I needed a language I was either familiar 
with or interested in, which among compiled languages meant either C, C++, 
or Rust.

I'd been interested in Rust for a few months at that point. 
A statically-typed, compiled language without having to worry about 
managing memory or inheritance? Sign me up please!


# Communication
The (first) big challenge I ran into was "how do I communicate the 
orientation to several consumers?" Specifically, how do I make the 
information available to multiple simultaneous consumers, who are going to 
come and go?

## Write to a File
My first thought was to just write to a file. Consumers could use inotify 
to know when the file changed and thus when the screen rotation needed 
changing.

### Pros:
-	Uses mature, stable code
-	Easy to consume
-	Doesn't require any extra work to provide to multiple consumers
-	Can use inotify to execute code on change

### Cons:
-	You're continually writing to a file, which is problematic if you can't 
	put it on a RAM-backed filesystem.
-	Not the most elegant solution
-	Does require the use of inotify to detect changes


## Events
My second thought was "Wait, couldn't I implement this as events?" 
I quickly concluded that *"Events are for drivers, Clu."*


## Dbus
So what about Dbus? Isn't it meant for IPC?

Well yes, it is; however, from what I gathered from my (admittedly somewhat 
cursory) look at it, it requires the sender know the recipient ahead of 
time. That, and I *really* don't want to have to implement responding to 
queries - this is spinnrd, not spinnrsrv.


## Use a Unix Domain Socket
Spinnrd is intended for Linux, right? So how about a socket?

This is a lovely idea, but while it's true that multiple consumers can 
connect to a single socket, it'd be more accurate to say that multiple 
consumers can use the same socket to establish 1-1 connections with the 
provider, which has to listen for those connections, establish them, and 
write to each of them individually after checking that they're still open 
(see previous comment about this not being spinnrsrv).

That, and Rust doesn't have any mature UDS libraries. [Dec. 2018]


## Use a (Network) Socket
All of the problems of a domain socket and more besides.  Gee, lemme 
think...


## Write to a Pipe
Okay, so how about a pipe?

While a pipe would alleviate the problem of writing to disk, they don't 
support multiple consumers. There'd need to be a pipe for each consumer, at 
which point it would be like using a socket, only clunkier and worse.


## Direct Memory Access
No. Just... **_no_**.


## Decision:
In the end, it seems that writing to a file is my best option, at least 
until someone comes up with a better way of intra-system info broadcasting 
for Linux. Or points me towards one I missed.


# Filename Formatting
`%d`: The spinnr directory (not for working dir)
`%e`: The current epoch time (in seconds)
`%_e`: The current epoch time (in milliseconds)
`%E`: The current UTC epoch time (in seconds.nanoseconds)
`%_E`: The current epoch time (in seconds.milliseconds)
`%f{FSTR}`: The current local date and time, formatted according to the 
	`strftime` string FSTR
`%F{FSTR}`: The current UTC date and time, formatted according to the 
	`strftime` string FSTR
`%t`: The current local date and time, in basic ISO 8601 format 
	(YYYYmmddTHHMMSS±hhmm)
`%_t`: The current local date and time, in basic ISO 8601 format with 
	nanoseconds (YYYYmmddTHHMMSS.NN±hhmm)
`%T`: The current UTC date and time, in basic ISO 8601 format 
	(YYYYmmddTHHMMSS±hhmm)
`%_T`: The current UTC date and time, in basic ISO 8601 format with 
	nanoseconds (YYYYmmddTHHMMSS.NN±hhmm)
`%%`: A literal '%'
`%}`: A literal '}' *(Doesn't end FSTR)*

## Later
These require some use of libc
`%u`: The name of the running user
`%_u`: The name of the calling user
`%U`: The UID of the running user
`%_U`: The UID of the calling user
`%g`: The name of the group
`%_g`: The name of the calling group
`%G`: The GID of the group
`%_G`: The GID of the calling group

## Feep
`%p`: The pid of the child process (spinfile only!)

# Opening the file to write it
This was something of a hard decision for me. It requires more work to do, 
but it makes inotifywait _much_ more feasible to use, so it's just the way 
I'm going to go.

# Command Line Options
-	no pid file
-	log level
-	daemonize
-	quiet
-	verbose
-	backend
-	backend options
-	delay
-	polling interval

## File locations
-	pid file
-	log file
-	spinfile
-	working directory

## Accelerometer options
### How to represent
Originally I thought to use an enum, calling a function to return an 
Orientator (filtered or not), but it'd be a lot less work to just use 
a ~~HashMap~~ ~~struct~~ HashMap.

### Universal options
-	~~hysteresis~~ *Moved to being a normal command line option.*

### FSAccel options
-	location of accelerometer files
-	file prefixes
-	file suffixes
-	x,y,z,scale filenames *x,y,z filenames as yet unimplemented*
-	default scale
-	override scale
-	fix int-as-uint

### Specifying backend
One of:
-	`<backend_name>[,[option]...]`
-	subcommand
-		but then how do I specify failover?

Going with `backend[[,option=value]...][;backend[[,option=value]...]]`.

# Building accelerometers
I could have used a builder struct, but would be overly much work for 
something that's only getting built once. Maybe if this were a library...  
but it's not.

# Passing backends
I've cracked it! Use an enum, with typedef'd ~~values~~ types, with the 
typedefs `cfg`-gated. Have a `DummyOrientator` struct, implementing the 
`Orientator` trait (just returns None).
