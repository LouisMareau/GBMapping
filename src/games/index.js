// Which game profile fits a ROM: by its header's game code. Each profile
// implements the interface described in pokeemerald.js.
'use strict';

import { PokeemeraldGame } from './pokeemerald.js';

export const PROFILES = [PokeemeraldGame];

export function detectGame(rom, profile = {}) {
  const header = rom.header();
  if (profile.game) {
    const named = PROFILES.find((p) => p.name.toLowerCase().startsWith(profile.game.toLowerCase()));
    if (named) return new named(rom, profile);
  }
  for (const P of PROFILES) if (P.matches(header)) return new P(rom, profile);
  return null;
}
