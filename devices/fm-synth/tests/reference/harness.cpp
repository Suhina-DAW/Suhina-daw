// SPDX-License-Identifier: GPL-3.0-or-later
// Reference renderer: plays the scenarios in scenarios.txt through the original MSFA engine
// (as found in Dexed), with the same voice handling as Suhina's FM synth, and writes each
// render as little-endian f32 (every `decimate`-th frame) for the port's golden tests.
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <memory>
#include <new>
#include <sstream>
#include <string>
#include <vector>

#include "msfa/synth.h"
#include "msfa/sin.h"
#include "msfa/exp2.h"
#include "msfa/freqlut.h"
#include "msfa/lfo.h"
#include "msfa/pitchenv.h"
#include "msfa/env.h"
#include "msfa/porta.h"
#include "msfa/controllers.h"
#include "msfa/fm_core.h"
#include "msfa/dx7note.h"

struct Event { int frame; std::string kind; int a; int b; };
struct Scenario {
    std::string name; double rate = 48000; int frames = 0; int decimate = 1;
    uint8_t patch[156] = {0};
    int perf[13] = {0};
    std::vector<Event> events;
};

static const int VOICES = 16;

struct Slot {
    Dx7Note *note; int key = -1; int sounding = -1; int velocity = 0;
    bool down = false, sustained = false, live = false; long seq = -1;
};

struct Synth {
    std::shared_ptr<TuningState> tuning = std::make_shared<TuningState>();
    FmCore core;
    Controllers c;
    Lfo lfo = Lfo();
    Slot slots[VOICES];
    long next_seq = 0; int last = -1; bool sustain = false;
    uint8_t patch[156];

    Synth(const Scenario &s) {
        memcpy(patch, s.patch, 156);
        for (auto &slot : slots) {
            void *mem = calloc(1, sizeof(Dx7Note));
            slot.note = new (mem) Dx7Note(tuning, nullptr);
        }
        c.pitch_cc = 0x2000;
        c.pitch_range_up = c.pitch_range_dn = s.perf[0];
        c.pitch_step = s.perf[1];
        c.portamento_enable_cc = s.perf[2] != 0;
        c.portamento_gliss_cc = s.perf[3] != 0;
        c.portamento_cc = (int)floor(s.perf[4] * 127.0 / 99.0 + 0.5);
        c.masterTune = 0;
        c.mpeEnabled = false;
        c.modwheel_cc = c.breath_cc = c.foot_cc = c.aftertouch_cc = 0;
        c.wheel.range = s.perf[5]; c.wheel.pitch = s.perf[6]; c.wheel.amp = s.perf[7]; c.wheel.eg = s.perf[8];
        c.at.range = s.perf[9]; c.at.pitch = s.perf[10]; c.at.amp = s.perf[11]; c.at.eg = s.perf[12];
        c.core = &core;
        c.refresh();
        lfo.reset(patch + 137);
    }

    // The voice already sounding the pitch, then a silent one, then the oldest released one,
    // then the oldest; as the port picks them.
    int first_started(int which, int sounding) {
        int best = -1;
        for (int i = 0; i < VOICES; i++) {
            Slot &s = slots[i];
            bool playing = s.live && s.note->isPlaying();
            bool ok = which == 0 ? (playing && s.sounding == sounding)
                    : which == 1 ? !playing
                    : which == 2 ? (!s.down && !s.sustained)
                    : true;
            if (ok && (best < 0 || s.seq < slots[best].seq)) best = i;
        }
        return best;
    }

    void key_down(int key, int velocity) {
        int sounding = key + patch[144] - 24;
        if (sounding < 0) sounding = 0;
        if (sounding > 127) sounding = 127;
        bool any_down = false;
        for (auto &s : slots) any_down = any_down || s.down;
        if (!any_down) lfo.keydown();
        int index = -1;
        for (int which = 0; which < 4 && index < 0; which++) index = first_started(which, sounding);
        bool glide = c.portamento_enable_cc && c.portamento_cc > 0;
        Slot &s = slots[index];
        bool was_sounding = s.live && s.note->isPlaying();
        s.key = key; s.sounding = sounding; s.velocity = velocity; s.sustained = sustain; s.down = true; s.seq = next_seq++;
        s.note->init(patch, sounding, velocity, 1, &c);
        if (patch[136] != 0 && !was_sounding) s.note->oscSync();
        if (glide && last >= 0 && last != index && slots[last].sounding >= 0) s.note->initPortamento(*slots[last].note);
        s.live = true;
        last = index;
    }

    void key_up(int key) {
        for (auto &s : slots) {
            if (s.down && s.key == key) {
                s.down = false;
                if (sustain) s.sustained = true; else s.note->keyup();
                return;
            }
        }
    }

    void apply(const Event &e) {
        if (e.kind == "on") { if (e.b == 0) key_up(e.a); else key_down(e.a, e.b); }
        else if (e.kind == "off") key_up(e.a);
        else if (e.kind == "bend") c.pitch_cc = e.a;
        else if (e.kind == "wheel") { c.modwheel_cc = e.a; c.refresh(); }
        else if (e.kind == "pressure") { c.aftertouch_cc = e.a; c.refresh(); }
        else if (e.kind == "sustain") {
            sustain = e.a != 0;
            if (!sustain) for (auto &s : slots) if (s.sustained && !s.down) { s.note->keyup(); s.sustained = false; }
        }
    }

    void render(float *out) {
        int32_t lfo_val = lfo.getsample();
        int32_t lfo_delay = lfo.getdelay();
        float sum[N] = {0};
        for (auto &s : slots) {
            if (!s.live || !s.note->isPlaying()) continue;
            int32_t buf[N] = {0};
            s.note->compute(buf, lfo_val, lfo_delay, &c);
            for (int j = 0; j < N; j++) {
                float f = (float)buf[j] / (float)(1u << 28);
                if (f > 1) f = 1;
                if (f < -1) f = -1;
                sum[j] += f;
            }
        }
        memcpy(out, sum, sizeof(sum));
    }
};

static std::vector<Scenario> parse(const char *path) {
    std::ifstream in(path);
    std::vector<Scenario> out;
    std::string line;
    Scenario cur;
    while (std::getline(in, line)) {
        std::istringstream ls(line);
        std::string word;
        if (!(ls >> word) || word[0] == '#') continue;
        if (word == "scenario") { cur = Scenario(); ls >> cur.name >> cur.rate >> cur.frames >> cur.decimate; }
        else if (word == "patch") { for (int i = 0; i < 156; i++) { int v; ls >> v; cur.patch[i] = (uint8_t)v; } }
        else if (word == "perf") { for (int i = 0; i < 13; i++) ls >> cur.perf[i]; }
        else if (word == "event") { Event e; e.a = e.b = 0; ls >> e.frame >> e.kind >> e.a; ls >> e.b; cur.events.push_back(e); }
        else if (word == "end") out.push_back(cur);
    }
    return out;
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: harness scenarios.txt outdir\n"); return 2; }
    Exp2::init();
    Sin::init();
    for (const Scenario &s : parse(argv[1])) {
        Freqlut::init(s.rate);
        Lfo::init(s.rate);
        PitchEnv::init(s.rate);
        Env::init_sr(s.rate);
        Porta::init_sr(s.rate);
        Synth synth(s);
        std::vector<float> all;
        size_t next = 0;
        for (int start = 0; start < s.frames; start += N) {
            while (next < s.events.size() && s.events[next].frame <= start) synth.apply(s.events[next++]);
            float block[N];
            synth.render(block);
            for (int j = 0; j < N && start + j < s.frames; j++) all.push_back(block[j]);
        }
        std::string path = std::string(argv[2]) + "/" + s.name + ".f32";
        FILE *f = fopen(path.c_str(), "wb");
        for (size_t i = 0; i < all.size(); i += s.decimate) fwrite(&all[i], 4, 1, f);
        fclose(f);
        printf("%s: %zu frames\n", s.name.c_str(), all.size());
    }
    return 0;
}
