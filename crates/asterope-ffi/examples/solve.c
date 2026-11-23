/*
 * Solves a sequence of raw frames from C.
 *
 *   solve <catalog.astc> <index.asti> <width> <height> <fov deg> <frame.raw>...
 *
 * Frames are headerless little-endian 16-bit pixels, top row first, as
 * written by `asterope simulate --out frame.raw`.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#include "asterope.h"

static uint8_t *read_file(const char *path, size_t *length) {
    FILE *file = fopen(path, "rb");
    if (!file) {
        return NULL;
    }
    fseek(file, 0, SEEK_END);
    long size = ftell(file);
    fseek(file, 0, SEEK_SET);
    uint8_t *bytes = size > 0 ? malloc((size_t)size) : NULL;
    if (bytes && fread(bytes, 1, (size_t)size, file) != (size_t)size) {
        free(bytes);
        bytes = NULL;
    }
    fclose(file);
    *length = bytes ? (size_t)size : 0;
    return bytes;
}

int main(int argc, char **argv) {
    if (argc < 7) {
        fprintf(stderr, "usage: %s <catalog.astc> <index.asti> <width> <height> <fov deg> <frame.raw>...\n", argv[0]);
        return 2;
    }
    size_t catalog_length, index_length;
    uint8_t *catalog = read_file(argv[1], &catalog_length);
    uint8_t *index = read_file(argv[2], &index_length);
    if (!catalog || !index) {
        fprintf(stderr, "cannot read the catalog or the index\n");
        return 1;
    }
    AsteropeCamera camera = {0};
    camera.width = (uint32_t)strtoul(argv[3], NULL, 10);
    camera.height = (uint32_t)strtoul(argv[4], NULL, 10);
    camera.fov_x_deg = strtod(argv[5], NULL);

    AsteropeTracker *tracker = NULL;
    AsteropeStatus status = asterope_tracker_new(&camera, catalog, catalog_length, index, index_length, &tracker);
    free(catalog); /* decoded into the tracker; the index must stay */
    if (status != ASTEROPE_STATUS_OK) {
        fprintf(stderr, "asterope_tracker_new: %s\n", asterope_status_message(status));
        return 1;
    }

    static const char *const EVENTS[] = {"ACQUIRE", "TRACK", "COAST", "LOST"};
    int failures = 0;
    for (int i = 6; i < argc; i++) {
        size_t length;
        uint8_t *frame = read_file(argv[i], &length);
        if (!frame) {
            fprintf(stderr, "cannot read %s\n", argv[i]);
            failures++;
            continue;
        }
        AsteropeFix fix;
        /* The file is little-endian; so is every platform this example targets. */
        status = asterope_tracker_update(tracker, (const uint16_t *)frame, length / 2, 0.1, &fix);
        free(frame);
        if (status != ASTEROPE_STATUS_OK) {
            fprintf(stderr, "%s: %s\n", argv[i], asterope_status_message(status));
            failures++;
            continue;
        }
        if (!fix.solved) {
            printf("%-8s %s\n", EVENTS[fix.event], argv[i]);
            continue;
        }
        printf("%-8s q %+.9f %+.9f %+.9f %+.9f  RA %.4f Dec %+.4f Roll %.4f  %u stars  %.2f\" rms\n",
               EVENTS[fix.event], fix.q[0], fix.q[1], fix.q[2], fix.q[3], fix.ra_deg, fix.dec_deg, fix.roll_deg,
               fix.matches, fix.rms_arcsec);
    }

    AsteropeMatch brightest[5];
    size_t matched = asterope_tracker_matches(tracker, brightest, 5);
    for (size_t i = 0; i < matched && i < 5; i++) {
        printf("  HIP %-6u V %.2f  %.1f\"\n", brightest[i].hip, brightest[i].magnitude, brightest[i].residual_arcsec);
    }

    asterope_tracker_free(tracker);
    free(index);
    return failures ? 1 : 0;
}
