# Sanic : A toy fast udp-based file transfer protocol

Assumption:
- For simplicity’s sake, we will assume that the MTU is 1500 octets.
- The whole protocol is mono-client: We can only receive from one client a time, and we can only send to one client at a time.
