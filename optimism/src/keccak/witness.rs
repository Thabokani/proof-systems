use super::{
    column::KeccakColumn,
    environment::KeccakEnv,
    interpreter::{Absorb, KeccakInterpreter, KeccakStep, Sponge},
    lookups::Lookups,
    DIM, HASH_BYTELENGTH, QUARTERS, WORDS_IN_HASH,
};
use ark_ff::Field;
use kimchi::{
    circuits::polynomials::keccak::{
        constants::{CAPACITY_IN_BYTES, RATE_IN_BYTES, ROUNDS, STATE_LEN},
        witness::{Chi, Iota, PiRho, Theta},
        Keccak,
    },
    grid,
};

pub(crate) fn pad_blocks<Fp: Field>(pad_bytelength: usize) -> Vec<Fp> {
    // Blocks to store padding. The first one uses at most 12 bytes, and the rest use at most 31 bytes.
    let mut blocks = vec![Fp::zero(); 5];
    let mut pad = [Fp::zero(); RATE_IN_BYTES];
    pad[RATE_IN_BYTES - pad_bytelength] = Fp::one();
    pad[RATE_IN_BYTES - 1] += Fp::from(0x80u8);
    blocks[0] = pad
        .iter()
        .take(12)
        .fold(Fp::zero(), |acc, x| acc * Fp::from(256u32) + *x);
    for (i, block) in blocks.iter_mut().enumerate().take(5).skip(1) {
        // take 31 elements from pad, starting at 12 + (i - 1) * 31 and fold them into a single Fp
        *block = pad
            .iter()
            .skip(12 + (i - 1) * 31)
            .take(31)
            .fold(Fp::zero(), |acc, x| acc * Fp::from(256u32) + *x);
    }

    blocks
}

impl<Fp: Field> KeccakInterpreter for KeccakEnv<Fp> {
    type Position = KeccakColumn;

    type Variable = Fp;

    fn hash(&mut self, preimage: Vec<u8>) {
        // TODO: Read preimage for each block

        self.blocks_left_to_absorb = Keccak::num_blocks(preimage.len()) as u64;

        // Configure first step depending on number of blocks remaining
        self.keccak_step = if self.blocks_left_to_absorb == 1 {
            Some(KeccakStep::Sponge(Sponge::Absorb(Absorb::FirstAndLast)))
        } else {
            Some(KeccakStep::Sponge(Sponge::Absorb(Absorb::First)))
        };
        self.step_counter = 0;

        // Root state is zero
        self.prev_block = vec![0u64; STATE_LEN];

        // Pad preimage
        self.padded = Keccak::pad(&preimage);
        self.block_idx = 0;
        self.pad_len = (self.padded.len() - preimage.len()) as u64;

        // Run all steps of hash
        while self.keccak_step.is_some() {
            self.step();
        }

        // TODO: create READ lookup tables
        // TODO: When finish, write hash to Syscall channel using `output_of_step()` on Squeeze step
    }

    // FIXME: read preimage from memory and pad and expand
    fn step(&mut self) {
        // Reset columns to zeros to avoid conflicts between steps
        self.null_state();

        // FIXME sparse notation

        match self.keccak_step.unwrap() {
            KeccakStep::Sponge(typ) => self.run_sponge(typ),
            KeccakStep::Round(i) => self.run_round(i),
        }
        self.write_column(KeccakColumn::StepCounter, self.step_counter);

        // INTER-STEP CHANNEL
        // Write outputs for next step if not a squeeze and read inputs of curr step if not a root
        self.lookup_steps();

        self.update_step();
    }

    fn set_flag_root(&mut self) {
        self.write_column(KeccakColumn::FlagRoot, 1);
    }

    fn set_flag_pad(&mut self) {
        self.write_column(KeccakColumn::FlagPad, 1);
        self.write_column(KeccakColumn::FlagLength, self.pad_len);
        let pad_range = RATE_IN_BYTES - self.pad_len as usize..RATE_IN_BYTES;
        for i in pad_range {
            self.write_column(KeccakColumn::FlagsBytes(i), 1);
        }
    }

    fn set_flag_absorb(&mut self, absorb: Absorb) {
        self.write_column(KeccakColumn::FlagAbsorb, 1);
        match absorb {
            Absorb::First => self.set_flag_root(),
            Absorb::Last => self.set_flag_pad(),
            Absorb::FirstAndLast => {
                self.set_flag_root();
                self.set_flag_pad()
            }
            Absorb::Middle => (),
        }
    }

    fn set_flag_round(&mut self, round: u64) {
        assert!(round <= ROUNDS as u64);
        // Values between 0 (dummy, for sponges) and 24
        self.write_column(KeccakColumn::FlagRound, round);
        if round != 0 {
            self.write_column_field(
                KeccakColumn::InverseRound,
                Fp::from(round).inverse().unwrap(),
            );
        }
    }

    fn run_sponge(&mut self, sponge: Sponge) {
        match sponge {
            Sponge::Absorb(absorb) => self.run_absorb(absorb),
            Sponge::Squeeze => self.run_squeeze(),
        }
    }

    fn run_squeeze(&mut self) {
        self.write_column(KeccakColumn::FlagSqueeze, 1);

        // Compute witness values
        let state = self.prev_block.clone();
        let shifts = Keccak::shift(&state);
        let dense = Keccak::collapse(&Keccak::reset(&shifts));
        let bytes = Keccak::bytestring(&dense);

        // Write squeeze-related columns
        for (i, value) in state.iter().enumerate() {
            self.write_column(KeccakColumn::SpongeOldState(i), *value);
        }
        for (i, value) in bytes.iter().enumerate().take(HASH_BYTELENGTH) {
            self.write_column(KeccakColumn::SpongeBytes(i), *value);
        }
        for (i, value) in shifts.iter().enumerate().take(QUARTERS * WORDS_IN_HASH) {
            self.write_column(KeccakColumn::SpongeShifts(i), *value);
        }

        // Rest is zero thanks to null_state

        // TODO: more updates to the env?
    }

    fn run_absorb(&mut self, absorb: Absorb) {
        self.set_flag_absorb(absorb);

        // Compute witness values
        let ini_idx = self.block_idx * RATE_IN_BYTES;
        let mut block = self.padded[ini_idx..ini_idx + RATE_IN_BYTES].to_vec();

        // Pad with zeros
        block.append(&mut vec![0; CAPACITY_IN_BYTES]);

        //    Round + Mode of Operation (Sponge)
        //    state -> permutation(state) -> state'
        //              ----> either [0] or state'
        //             |            new state = Exp(block)
        //             |         ------------------------
        //    Absorb: state  + [  block      +     0...0 ]
        //                       1088 bits          512
        //            ----------------------------------
        //                         XOR STATE
        let old_state = self.prev_block.clone();
        let new_state = Keccak::expand_state(&block);
        let xor_state = old_state
            .iter()
            .zip(new_state.clone())
            .map(|(x, y)| x + y)
            .collect::<Vec<u64>>();

        let shifts = Keccak::shift(&new_state);
        let bytes = block.iter().map(|b| *b as u64).collect::<Vec<u64>>();

        // Write absorb-related columns
        for i in 0..QUARTERS * DIM * DIM {
            self.write_column(KeccakColumn::SpongeOldState(i), old_state[i]);
            self.write_column(KeccakColumn::SpongeNewState(i), new_state[i]);
            self.write_column(KeccakColumn::SpongeXorState(i), xor_state[i]);
        }
        for (i, value) in bytes.iter().enumerate() {
            self.write_column(KeccakColumn::SpongeBytes(i), *value);
        }
        for (i, value) in shifts.iter().enumerate() {
            self.write_column(KeccakColumn::SpongeShifts(i), *value);
        }
        let pad_blocks = pad_blocks::<Fp>(self.pad_len as usize);
        for (i, value) in pad_blocks.iter().enumerate() {
            self.write_column_field(KeccakColumn::PadSuffix(i), *value);
        }
        // Rest is zero thanks to null_state

        // Update environment
        self.prev_block = xor_state;
        self.block_idx += 1; // To be used in next absorb (if any)
    }

    fn run_round(&mut self, round: u64) {
        self.set_flag_round(round);

        let state_a = self.prev_block.clone();
        let state_e = self.run_theta(&state_a);
        let state_b = self.run_pirho(&state_e);
        let state_f = self.run_chi(&state_b);
        let state_g = self.run_iota(&state_f, round as usize);

        // Update block for next step with the output of the round
        self.prev_block = state_g;
    }

    fn run_theta(&mut self, state_a: &[u64]) -> Vec<u64> {
        let theta = Theta::create(state_a);

        // Write Theta-related columns
        for x in 0..DIM {
            self.write_column(KeccakColumn::ThetaQuotientC(x), theta.quotient_c(x));
            for q in 0..QUARTERS {
                self.write_column(KeccakColumn::ThetaDenseC(x, q), theta.dense_c(x, q));
                self.write_column(KeccakColumn::ThetaRemainderC(x, q), theta.remainder_c(x, q));
                self.write_column(KeccakColumn::ThetaDenseRotC(x, q), theta.dense_rot_c(x, q));
                self.write_column(
                    KeccakColumn::ThetaExpandRotC(x, q),
                    theta.expand_rot_c(x, q),
                );
                for y in 0..DIM {
                    let state_a = grid!(100, state_a);
                    self.write_column(KeccakColumn::ThetaStateA(y, x, q), state_a(y, x, q));
                }
                for i in 0..QUARTERS {
                    self.write_column(KeccakColumn::ThetaShiftsC(i, x, q), theta.shifts_c(i, x, q));
                }
            }
        }
        theta.state_e()
    }

    fn run_pirho(&mut self, state_e: &[u64]) -> Vec<u64> {
        let pirho = PiRho::create(state_e);

        // Write PiRho-related columns
        for y in 0..DIM {
            for x in 0..DIM {
                for q in 0..QUARTERS {
                    self.write_column(KeccakColumn::PiRhoDenseE(y, x, q), pirho.dense_e(y, x, q));
                    self.write_column(
                        KeccakColumn::PiRhoQuotientE(y, x, q),
                        pirho.quotient_e(y, x, q),
                    );
                    self.write_column(
                        KeccakColumn::PiRhoRemainderE(y, x, q),
                        pirho.remainder_e(y, x, q),
                    );
                    self.write_column(
                        KeccakColumn::PiRhoDenseRotE(y, x, q),
                        pirho.dense_rot_e(y, x, q),
                    );
                    self.write_column(
                        KeccakColumn::PiRhoExpandRotE(y, x, q),
                        pirho.expand_rot_e(y, x, q),
                    );
                    for i in 0..QUARTERS {
                        self.write_column(
                            KeccakColumn::PiRhoShiftsE(i, y, x, q),
                            pirho.shifts_e(i, y, x, q),
                        );
                    }
                }
            }
        }
        pirho.state_b()
    }

    fn run_chi(&mut self, state_b: &[u64]) -> Vec<u64> {
        let chi = Chi::create(state_b);

        // Write Chi-related columns
        for i in 0..DIM {
            for y in 0..DIM {
                for x in 0..DIM {
                    for q in 0..QUARTERS {
                        self.write_column(
                            KeccakColumn::ChiShiftsB(i, y, x, q),
                            chi.shifts_b(i, y, x, q),
                        );
                        self.write_column(
                            KeccakColumn::ChiShiftsSum(i, y, x, q),
                            chi.shifts_sum(i, y, x, q),
                        );
                    }
                }
            }
        }
        chi.state_f()
    }

    fn run_iota(&mut self, state_f: &[u64], round: usize) -> Vec<u64> {
        let iota = Iota::create(state_f, round);
        let state_g = iota.state_g();

        // Update columns
        for (i, g) in state_g.iter().enumerate() {
            self.write_column(KeccakColumn::IotaStateG(i), *g);
        }
        for i in 0..QUARTERS {
            self.write_column(KeccakColumn::RoundConstants(i), iota.rc(i));
        }

        state_g
    }
}
