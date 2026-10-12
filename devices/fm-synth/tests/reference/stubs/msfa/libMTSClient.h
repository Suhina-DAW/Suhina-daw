// Stub for the reference build: no MTS-ESP master.
#pragma once
struct MTSClient {};
inline bool MTS_HasMaster(MTSClient *) { return false; }
inline double MTS_NoteToFrequency(MTSClient *, char, char) { return 0.0; }
