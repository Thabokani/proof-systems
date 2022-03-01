use super::Mode;
use ark_ff::{fields::PrimeField as _, UniformRand as _};
use ark_serialize::CanonicalSerialize as _;
use mina_curves::pasta::Fp;
use num_bigint::BigUint;
use oracle::poseidon::ArithmeticSponge as Poseidon;
use oracle::poseidon::Sponge as _;
use rand::prelude::*;
use rand::Rng;
use serde::Serialize;

//
// generate different test vectors depending on features
//

use oracle::pasta::fp as SpongeParameters;
#[cfg(all(feature = "basic", not(feature = "15w")))]
use oracle::poseidon::PlonkSpongeConstantsBasic as PlonkSpongeConstants;

#[cfg(feature = "15w")]
use oracle::poseidon::PlonkSpongeConstants15W as PlonkSpongeConstants;

//
// structs
//

#[derive(Debug, Serialize)]
pub struct TestVectors {
    name: String,
    test_vectors: Vec<TestVector>,
}

#[derive(Debug, Serialize)]
pub struct TestVector {
    input: Vec<String>,
    output: String,
}

//
// logic
//

/// Computes the poseidon hash of several field elements.
/// Uses the 'basic' configuration with N states and M rounds.
fn poseidon(input: &[Fp]) -> Fp {
    let mut s = Poseidon::<Fp, PlonkSpongeConstants>::new(SpongeParameters::params());
    s.absorb(input);

    s.squeeze()
}

/// generates a vector of `length` field elements
fn rand_fields(rng: &mut impl Rng, length: u8) -> Vec<Fp> {
    let mut fields = vec![];
    for _ in 0..length {
        let fe = Fp::rand(rng);
        fields.push(fe)
    }
    fields
}

/// creates a set of test vectors
pub fn generate(mode: Mode) -> TestVectors {
    let mut rng = &mut rand::rngs::StdRng::from_seed([0u8; 32]);
    let mut test_vectors = vec![];

    // generate inputs of different lengths
    for length in 0..6 {
        // generate input & hash
        let input = rand_fields(&mut rng, length);
        let output = poseidon(&input);

        // serialize input & output
        let input = input
            .into_iter()
            .map(|elem| {
                let mut input_bytes = vec![];
                elem.into_repr()
                    .serialize(&mut input_bytes)
                    .expect("canonical serialiation should work");
                match mode {
                    Mode::Hex => hex::encode(&input_bytes),
                    Mode::B10 => BigUint::from_bytes_le(&input_bytes).to_string(),
                }
            })
            .collect();
        let mut output_bytes = vec![];
        output
            .into_repr()
            .serialize(&mut output_bytes)
            .expect("canonical serialization should work");

        // add vector
        test_vectors.push(TestVector {
            input,
            output: hex::encode(&output_bytes),
        })
    }

    let name = if cfg!(feature = "basic") {
        "basic".to_string()
    } else if cfg!(feature = "15w") {
        "15w".to_string()
    } else {
        panic!("test vector feature not recognized");
    };

    //
    TestVectors { name, test_vectors }
}
