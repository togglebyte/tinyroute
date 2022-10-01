# TinyRoute

A message routing library.

**Note** this is a work in progress!

## What is all this then?

Working with multiple channels can easily lead to complex code and tracking all
the `Sender`s and `Receiver`s becomes more difficult.

Instead of passing `Sender`s and `Receiver`s around, messages can be sent to
`Agent`s via `Address`es.
This means it's not necessary to create another `mpsc` pair to communicate.
