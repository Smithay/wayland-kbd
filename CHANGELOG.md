# Change Log

## 0.13.0 - 2017-10-29

- Update to wayland-client 0.12

## 0.12.0 - 2017-10-17

- API change: nothing is stored in the event queue state
- Add a method to initialize a mapped keyboard with provided RMLVO
- Take dead keys and compose into account

## 0.11.0 - 2017-10-10

Update dependencies wayland-client to 0.11, dlib to 0.4 and bitflags to 1.0

## 0.10.0 - 2017-09-19

Update the lib to wayland-client-0.10

## 0.9.1 - 2017-01-02

Migrate the repo to smithay org

## 0.9.0 - 2017-05-29

0.8.1 should have been 0.9.0

## 0.8.1 - 2017-03-19 [yanked]

wayland-kbd is compatible with wayland-client-0.9.x

## 0.8.0 - 2017-03-02

Update wayland-client dependency

## 0.7.0 - 2017-02-12

Track modifier keys state and expose them in callbacks

## 0.6.2 - 2016-10-09

MappedKeyboard is now Send.

## 0.6.1 - 2016-10-09

Add a handler() method to access the underlying handler

## 0.6.0 - 2016-10-04

Expose raw keycode too in handlers

## 0.5.0 - 2016-10-03

Update to wayland-client-0.7

## 0.4.0 - 2016-05-29

Update to wayland-client-0.6

## 0.3.6 - 2016-05-15

Update bitflags to 0.7.

## 0.3.5 - 2015-11-19

Update dlib and wayland-client dependencies.

## 0.3.4 - 2016-01-16

Switched from rust-mmap to memmap-rs.

## 0.3.3 - 2015-12-13

Made `KayEvent.keycode` public, for advanced uses.

## 0.3.2 - 2015-12-09

Update wayland-client to 0.5.

## 0.3.1 - 2015-11-19

Update dlib dependency.

## 0.3.0 - 2015-11-19

Update the library to the new wayland-client API.
