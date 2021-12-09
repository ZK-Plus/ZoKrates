use fallible_iterator::{FallibleIterator, FromFallibleIterator, IntoFallibleIterator};
use std::iter::FromIterator;

#[derive(Clone, PartialEq, Debug, Hash)]
pub struct MemoryStatements<S>(pub Vec<S>);

impl<S> Default for MemoryStatements<S> {
    fn default() -> Self {
        MemoryStatements(vec![])
    }
}

pub type DynamicError = Box<dyn std::error::Error>;

impl<S> From<Vec<S>> for MemoryStatements<S> {
    fn from(v: Vec<S>) -> Self {
        MemoryStatements(v)
    }
}

impl<S> MemoryStatements<S> {
    pub fn iter(&self) -> std::slice::Iter<S> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_inner(self) -> Vec<S> {
        self.0
    }
}

pub trait StatementTrait {
    type Field;
}

pub trait IntoStatements:
    IntoFallibleIterator<Item = Self::Statement, Error = DynamicError>
{
    type Statement: StatementTrait;
    type Field;
}

impl<S: StatementTrait, U: IntoFallibleIterator<Item = S, Error = DynamicError>> IntoStatements
    for U
{
    type Statement = S;
    type Field = S::Field;
}

pub trait Statements: FallibleIterator<Item = Self::Statement, Error = DynamicError> {
    type Statement: StatementTrait;
    type Field;
}

impl<S: StatementTrait, U: FallibleIterator<Item = S, Error = DynamicError>> Statements for U {
    type Statement = S;
    type Field = S::Field;
}

impl<S> IntoIterator for MemoryStatements<S> {
    type Item = S;
    type IntoIter = std::vec::IntoIter<S>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<S> FromIterator<S> for MemoryStatements<S> {
    fn from_iter<I: IntoIterator<Item = S>>(i: I) -> Self {
        MemoryStatements(i.into_iter().collect())
    }
}

impl<S: StatementTrait> FromFallibleIterator<S> for MemoryStatements<S> {
    fn from_fallible_iter<I: IntoFallibleIterator<Item = S>>(i: I) -> Result<Self, I::Error> {
        Ok(MemoryStatements(i.into_fallible_iter().collect()?))
    }
}

pub struct MemoryStatementsIterator<S, I: Iterator<Item = S>> {
    statements: I,
}

impl<S, I: Iterator<Item = S>> FallibleIterator for MemoryStatementsIterator<S, I> {
    type Item = S;
    type Error = DynamicError;

    fn next(&mut self) -> Result<Option<Self::Item>, Self::Error> {
        Ok(self.statements.next())
    }
}

impl<S> IntoFallibleIterator for MemoryStatements<S> {
    type Item = S;
    type Error = DynamicError;
    type IntoFallibleIter = MemoryStatementsIterator<S, std::vec::IntoIter<S>>;

    fn into_fallible_iter(self) -> Self::IntoFallibleIter {
        MemoryStatementsIterator {
            statements: self.into_iter(),
        }
    }
}
