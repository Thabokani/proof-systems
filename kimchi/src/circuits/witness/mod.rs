use ark_ff::{Field, PrimeField};

mod constant_cell;
mod copy_bits_cell;
mod copy_cell;
mod copy_shift_cell;
mod index_cell;
mod variable_bits_cell;
mod variable_cell;
mod variables;

pub use self::{
    constant_cell::ConstantCell,
    copy_bits_cell::CopyBitsCell,
    copy_cell::CopyCell,
    copy_shift_cell::CopyShiftCell,
    index_cell::IndexCell,
    variable_bits_cell::VariableBitsCell,
    variable_cell::VariableCell,
    variables::{variable_map, variables, Variables},
};

/// Witness cell interface
pub trait WitnessCell<const N: usize, F: Field, T> {
    fn value(&self, witness: &mut [Vec<F>; N], variables: &Variables<T>, index: usize) -> F;

    fn length(&self) -> usize {
        1
    }
}

/// Initialize a witness cell based on layout and computed variables
pub fn init_cell<const N: usize, F: PrimeField, T>(
    witness: &mut [Vec<F>; N],
    offset: usize,
    row: usize,
    col: usize,
    index: usize,
    layout: &[Vec<Box<dyn WitnessCell<N, F, T>>>],
    variables: &Variables<T>,
) {
    witness[col + index][row + offset] = layout[row][col].value(witness, variables, index);
}

/// Initialize a witness row based on layout and computed variables
pub fn init_row<const N: usize, F: PrimeField, T>(
    witness: &mut [Vec<F>; N],
    offset: usize,
    row: usize,
    layout: &[Vec<Box<dyn WitnessCell<N, F, T>>>],
    variables: &Variables<T>,
) {
    for col in 0..layout[0].len() {
        for index in 0..layout[row][col].length() {
            init_cell(witness, offset, row, col, index, layout, variables);
        }
    }
}

/// Initialize a witness based on layout and computed variables
pub fn init<const N: usize, F: PrimeField, T>(
    witness: &mut [Vec<F>; N],
    offset: usize,
    layout: &[Vec<Box<dyn WitnessCell<N, F, T>>>],
    variables: &Variables<T>,
) {
    for row in 0..layout.len() {
        init_row(witness, offset, row, layout, variables);
    }
}

#[cfg(test)]
mod tests {
    use std::array;

    use super::*;

    use crate::circuits::polynomial::COLUMNS;
    use ark_ec::AffineCurve;
    use ark_ff::{Field, One, Zero};
    use mina_curves::pasta::Pallas;
    type PallasField = <Pallas as AffineCurve>::BaseField;

    #[test]
    fn zero_layout() {
        let layout: Vec<Vec<Box<dyn WitnessCell<COLUMNS, PallasField, PallasField>>>> = vec![vec![
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
            ConstantCell::create(PallasField::zero()),
        ]];

        let mut witness: [Vec<PallasField>; COLUMNS] =
            array::from_fn(|_| vec![PallasField::one(); 1]);

        for col in witness.clone() {
            for field in col {
                assert_eq!(field, PallasField::one());
            }
        }

        // Set a single cell to zero
        init_cell(&mut witness, 0, 0, 4, 0, &layout, &variables!());
        assert_eq!(witness[4][0], PallasField::zero());

        // Set all the cells to zero
        init_row(&mut witness, 0, 0, &layout, &variables!());

        for col in witness {
            for field in col {
                assert_eq!(field, PallasField::zero());
            }
        }
    }

    #[test]
    fn mixed_layout() {
        let layout: Vec<Vec<Box<dyn WitnessCell<COLUMNS, PallasField, PallasField>>>> = vec![
            vec![
                ConstantCell::create(PallasField::from(12u32)),
                ConstantCell::create(PallasField::from(0xa5a3u32)),
                ConstantCell::create(PallasField::from(0x800u32)),
                CopyCell::create(0, 0),
                CopyBitsCell::create(0, 1, 0, 4),
                CopyShiftCell::create(0, 2, 12),
                VariableCell::create("sum_of_products"),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
            ],
            vec![
                CopyCell::create(0, 0),
                CopyBitsCell::create(0, 1, 4, 8),
                CopyShiftCell::create(0, 2, 8),
                VariableCell::create("sum_of_products"),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                VariableCell::create("something_else"),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                ConstantCell::create(PallasField::zero()),
                VariableCell::create("final_value"),
            ],
        ];

        let mut witness: [Vec<PallasField>; COLUMNS] =
            array::from_fn(|_| vec![PallasField::zero(); 2]);

        // Local variable (witness computation) with same names as VariableCell above
        let sum_of_products = PallasField::from(1337u32);
        let something_else = sum_of_products * PallasField::from(5u32);
        let final_value = (something_else + PallasField::one()).pow([2u64]);

        init_row(
            &mut witness,
            0,
            0,
            &layout,
            &variables!(sum_of_products, something_else, final_value),
        );

        assert_eq!(witness[3][0], PallasField::from(12u32));
        assert_eq!(witness[4][0], PallasField::from(0x3u32));
        assert_eq!(witness[5][0], PallasField::from(0x800000u32));
        assert_eq!(witness[6][0], sum_of_products);

        init_row(
            &mut witness,
            0,
            1,
            &layout,
            &variables!(sum_of_products, something_else, final_value),
        );

        assert_eq!(witness[0][1], PallasField::from(12u32));
        assert_eq!(witness[1][1], PallasField::from(0xau32));
        assert_eq!(witness[2][1], PallasField::from(0x80000u32));
        assert_eq!(witness[3][1], sum_of_products);
        assert_eq!(witness[7][1], something_else);
        assert_eq!(witness[14][1], final_value);

        let mut witness2: [Vec<PallasField>; COLUMNS] =
            array::from_fn(|_| vec![PallasField::zero(); 2]);
        init(
            &mut witness2,
            0,
            &layout,
            &variables!(sum_of_products, something_else, final_value),
        );

        for row in 0..witness[0].len() {
            for col in 0..witness.len() {
                assert_eq!(witness[col][row], witness2[col][row]);
            }
        }
    }
}
