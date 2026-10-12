// Stub for the reference build: standard tuning only.
#ifndef __SYNTH_TUNING_H
#define __SYNTH_TUNING_H
#include "synth.h"
#include <memory>

class TuningState {
public:
    virtual ~TuningState() {}
    int32_t midinote_to_logfreq(int midinote) { return 50857777 + ((1 << 24) / 12) * midinote; }
    bool is_standard_tuning() { return true; }
};
#endif
