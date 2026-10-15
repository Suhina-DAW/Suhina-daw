# Feedback impulse oracle

`feedback.f32` contains 16 little-endian interleaved stereo frames:
left = 0.5^n, right = -0.5^n for n=0..15. It is the analytic response
of a direct unit impulse plus a one-frame delay with feedback 0.5.
The graph processor must reproduce it regardless of node storage order.
