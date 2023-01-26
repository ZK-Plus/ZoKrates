use crate::{
    ir::{check::UnconstrainedVariableDetector, solver_indexer::SolverIndexer},
    Solver,
};

use super::{ProgIterator, Statement};
use serde::Deserialize;
use serde_cbor::{self, StreamDeserializer};
use std::io::{Read, Seek, Write};
use zokrates_field::*;

type DynamicError = Box<dyn std::error::Error>;

const ZOKRATES_MAGIC: &[u8; 4] = &[0x5a, 0x4f, 0x4b, 0];
const ZOKRATES_VERSION_3: &[u8; 4] = &[0, 0, 0, 3];

#[derive(PartialEq, Eq, Debug)]
pub enum ProgEnum<
    'ast,
    Bls12_381I: IntoIterator<Item = Statement<'ast, Bls12_381Field>>,
    Bn128I: IntoIterator<Item = Statement<'ast, Bn128Field>>,
    Bls12_377I: IntoIterator<Item = Statement<'ast, Bls12_377Field>>,
    Bw6_761I: IntoIterator<Item = Statement<'ast, Bw6_761Field>>,
> {
    Bls12_381Program(ProgIterator<'ast, Bls12_381Field, Bls12_381I>),
    Bn128Program(ProgIterator<'ast, Bn128Field, Bn128I>),
    Bls12_377Program(ProgIterator<'ast, Bls12_377Field, Bls12_377I>),
    Bw6_761Program(ProgIterator<'ast, Bw6_761Field, Bw6_761I>),
}

type MemoryProgEnum<'ast> = ProgEnum<
    'ast,
    Vec<Statement<'ast, Bls12_381Field>>,
    Vec<Statement<'ast, Bn128Field>>,
    Vec<Statement<'ast, Bls12_377Field>>,
    Vec<Statement<'ast, Bw6_761Field>>,
>;

impl<
        'ast,
        Bls12_381I: IntoIterator<Item = Statement<'ast, Bls12_381Field>>,
        Bn128I: IntoIterator<Item = Statement<'ast, Bn128Field>>,
        Bls12_377I: IntoIterator<Item = Statement<'ast, Bls12_377Field>>,
        Bw6_761I: IntoIterator<Item = Statement<'ast, Bw6_761Field>>,
    > ProgEnum<'ast, Bls12_381I, Bn128I, Bls12_377I, Bw6_761I>
{
    pub fn collect(self) -> MemoryProgEnum<'ast> {
        match self {
            ProgEnum::Bls12_381Program(p) => ProgEnum::Bls12_381Program(p.collect()),
            ProgEnum::Bn128Program(p) => ProgEnum::Bn128Program(p.collect()),
            ProgEnum::Bls12_377Program(p) => ProgEnum::Bls12_377Program(p.collect()),
            ProgEnum::Bw6_761Program(p) => ProgEnum::Bw6_761Program(p.collect()),
        }
    }
    pub fn curve(&self) -> &'static str {
        match self {
            ProgEnum::Bn128Program(_) => Bn128Field::name(),
            ProgEnum::Bls12_381Program(_) => Bls12_381Field::name(),
            ProgEnum::Bls12_377Program(_) => Bls12_377Field::name(),
            ProgEnum::Bw6_761Program(_) => Bw6_761Field::name(),
        }
    }
}

impl<'ast, T: Field, I: IntoIterator<Item = Statement<'ast, T>>> ProgIterator<'ast, T, I> {
    /// serialize a program iterator, returning the number of constraints serialized
    /// Note that we only return constraints, not other statements such as directives
    pub fn serialize<W: Write + Seek>(self, mut w: W) -> Result<usize, DynamicError> {
        use super::folder::Folder;

        w.write_all(ZOKRATES_MAGIC)?;
        w.write_all(ZOKRATES_VERSION_3)?;
        w.write_all(&T::id())?;

        let solver_list_ptr_offset = w.stream_position()?;
        w.write_all(&[0u8; std::mem::size_of::<u64>()])?; // reserve 8 bytes

        serde_cbor::to_writer(&mut w, &self.arguments)?;
        serde_cbor::to_writer(&mut w, &self.return_count)?;

        let mut unconstrained_variable_detector = UnconstrainedVariableDetector::new(&self);
        let mut solver_indexer: SolverIndexer<'ast, T> = SolverIndexer::default();

        let statements = self.statements.into_iter();

        let mut count = 0;
        for s in statements {
            if matches!(s, Statement::Constraint(..)) {
                count += 1;
            }
            let s: Vec<Statement<T>> = solver_indexer
                .fold_statement(s)
                .into_iter()
                .flat_map(|s| unconstrained_variable_detector.fold_statement(s))
                .collect();
            for s in s {
                serde_cbor::to_writer(&mut w, &s)?;
            }
        }

        let solver_list_offset = w.stream_position()?;
        serde_cbor::to_writer(&mut w, &solver_indexer.solvers)?;

        w.seek(std::io::SeekFrom::Start(solver_list_ptr_offset))?;
        w.write_all(&solver_list_offset.to_le_bytes())?;

        unconstrained_variable_detector
            .finalize()
            .map(|_| count)
            .map_err(|count| format!("Error: Found {} unconstrained variable(s)", count).into())
    }
}

pub struct UnwrappedStreamDeserializer<'de, R, T> {
    s: StreamDeserializer<'de, R, T>,
}

impl<'de, R: serde_cbor::de::Read<'de>, T: serde::Deserialize<'de>> Iterator
    for UnwrappedStreamDeserializer<'de, R, T>
{
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.s.next().and_then(|v| v.ok())
    }
}

impl<'de, R: Read + Seek>
    ProgEnum<
        'de,
        UnwrappedStreamDeserializer<'de, serde_cbor::de::IoRead<R>, Statement<'de, Bls12_381Field>>,
        UnwrappedStreamDeserializer<'de, serde_cbor::de::IoRead<R>, Statement<'de, Bn128Field>>,
        UnwrappedStreamDeserializer<'de, serde_cbor::de::IoRead<R>, Statement<'de, Bls12_377Field>>,
        UnwrappedStreamDeserializer<'de, serde_cbor::de::IoRead<R>, Statement<'de, Bw6_761Field>>,
    >
{
    pub fn deserialize(mut r: R) -> Result<Self, String> {
        // Check the magic number, `ZOK`
        let mut magic = [0; 4];
        r.read_exact(&mut magic)
            .map_err(|_| String::from("Cannot read magic number"))?;

        if &magic == ZOKRATES_MAGIC {
            // Check the version, 2
            let mut version = [0; 4];
            r.read_exact(&mut version)
                .map_err(|_| String::from("Cannot read version"))?;

            if &version == ZOKRATES_VERSION_3 {
                // Check the curve identifier, deserializing accordingly
                let mut curve = [0; 4];
                r.read_exact(&mut curve)
                    .map_err(|_| String::from("Cannot read curve identifier"))?;

                let mut buffer = [0u8; std::mem::size_of::<u64>()];
                r.read_exact(&mut buffer)
                    .map_err(|_| String::from("Cannot read solver list offset"))?;

                let solver_list_offset = u64::from_le_bytes(buffer);

                let (arguments, return_count) = {
                    let mut p = serde_cbor::Deserializer::from_reader(r.by_ref());

                    let arguments: Vec<super::Parameter> = Vec::deserialize(&mut p)
                        .map_err(|_| String::from("Cannot read parameters"))?;

                    let return_count = usize::deserialize(&mut p)
                        .map_err(|_| String::from("Cannot read return count"))?;

                    (arguments, return_count)
                };

                let statement_offset = r.stream_position().unwrap();
                r.seek(std::io::SeekFrom::Start(solver_list_offset))
                    .unwrap();

                match curve {
                    m if m == Bls12_381Field::id() => {
                        let solvers: Vec<Solver<'de, Bls12_381Field>> = {
                            let mut p = serde_cbor::Deserializer::from_reader(r.by_ref());
                            Vec::deserialize(&mut p)
                                .map_err(|_| String::from("Cannot read solver list"))?
                        };

                        r.seek(std::io::SeekFrom::Start(statement_offset)).unwrap();

                        let p = serde_cbor::Deserializer::from_reader(r);
                        let s = p.into_iter::<Statement<Bls12_381Field>>();
                        Ok(ProgEnum::Bls12_381Program(ProgIterator::new(
                            arguments,
                            UnwrappedStreamDeserializer { s },
                            return_count,
                            solvers,
                        )))
                    }
                    m if m == Bn128Field::id() => {
                        let solvers: Vec<Solver<'de, Bn128Field>> = {
                            let mut p = serde_cbor::Deserializer::from_reader(r.by_ref());
                            Vec::deserialize(&mut p)
                                .map_err(|_| String::from("Cannot read solver list"))?
                        };

                        r.seek(std::io::SeekFrom::Start(statement_offset)).unwrap();

                        let p = serde_cbor::Deserializer::from_reader(r);
                        let s = p.into_iter::<Statement<Bn128Field>>();

                        Ok(ProgEnum::Bn128Program(ProgIterator::new(
                            arguments,
                            UnwrappedStreamDeserializer { s },
                            return_count,
                            solvers,
                        )))
                    }
                    m if m == Bls12_377Field::id() => {
                        let solvers: Vec<Solver<'de, Bls12_377Field>> = {
                            let mut p = serde_cbor::Deserializer::from_reader(r.by_ref());
                            Vec::deserialize(&mut p)
                                .map_err(|_| String::from("Cannot read solver list"))?
                        };

                        r.seek(std::io::SeekFrom::Start(statement_offset)).unwrap();

                        let p = serde_cbor::Deserializer::from_reader(r);
                        let s = p.into_iter::<Statement<Bls12_377Field>>();

                        Ok(ProgEnum::Bls12_377Program(ProgIterator::new(
                            arguments,
                            UnwrappedStreamDeserializer { s },
                            return_count,
                            solvers,
                        )))
                    }
                    m if m == Bw6_761Field::id() => {
                        let solvers: Vec<Solver<'de, Bw6_761Field>> = {
                            let mut p = serde_cbor::Deserializer::from_reader(r.by_ref());
                            Vec::deserialize(&mut p)
                                .map_err(|_| String::from("Cannot read solver list"))?
                        };

                        r.seek(std::io::SeekFrom::Start(statement_offset)).unwrap();

                        let p = serde_cbor::Deserializer::from_reader(r);
                        let s = p.into_iter::<Statement<Bw6_761Field>>();

                        Ok(ProgEnum::Bw6_761Program(ProgIterator::new(
                            arguments,
                            UnwrappedStreamDeserializer { s },
                            return_count,
                            solvers,
                        )))
                    }
                    _ => Err(String::from("Unknown curve identifier")),
                }
            } else {
                Err(String::from("Unknown version"))
            }
        } else {
            Err(String::from("Wrong magic number"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Prog;
    use std::io::{Cursor, Seek, SeekFrom};
    use zokrates_field::{Bls12_381Field, Bn128Field};

    #[test]
    fn ser_deser_v2() {
        let p: Prog<Bn128Field> = Prog::default();

        let mut buffer = Cursor::new(vec![]);
        p.clone().serialize(&mut buffer).unwrap();

        // rewind back to the beginning of the file
        buffer.seek(SeekFrom::Start(0)).unwrap();

        // deserialize
        let deserialized_p = ProgEnum::deserialize(buffer).unwrap();

        assert_eq!(ProgEnum::Bn128Program(p), deserialized_p.collect());

        let p: Prog<Bls12_381Field> = Prog::default();

        let mut buffer = Cursor::new(vec![]);
        p.clone().serialize(&mut buffer).unwrap();

        // rewind back to the beginning of the file
        buffer.seek(SeekFrom::Start(0)).unwrap();

        // deserialize
        let deserialized_p = ProgEnum::deserialize(buffer).unwrap();

        assert_eq!(ProgEnum::Bls12_381Program(p), deserialized_p.collect());
    }
}
