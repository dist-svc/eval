use std::{num::ParseIntError, str::FromStr, fmt::Display};



#[derive(Clone, Debug)]
pub enum IntRange {
    Val(Vec<usize>),
    Inc(usize, usize, usize),
}

impl IntRange {

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        IntRangeIter{range: self, index: 0}
    }
}

impl FromStr for IntRange {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let r = if s.contains(',') {
            let v = s.split(',').map(|v| usize::from_str(v) ).collect::<Result<Vec<_>, _>>()?;
            Self::Val(v)
        } else if s.contains(':') {
            let mut v = s.split(':');
            match (v.next(), v.next(), v.next()) {
                (Some(start), Some(end), Some(step)) => Self::Inc(usize::from_str(start)?, usize::from_str(end)?, usize::from_str(step)?),
                (Some(start), Some(end), None) => Self::Inc(usize::from_str(start)?, usize::from_str(end)?, 1),
                _ => unreachable!(),
            }
        } else {
            let v = usize::from_str(s)?;
            Self::Val(vec![v])
        };

        Ok(r)
    }
}

impl Display for IntRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Val(v) => write!(f, "{:?}", v),
            Self::Inc(start, end, step) => write!(f, "{{start: {start}, end: {end}, step: {step}}}"),
        }
    }
}

pub struct IntRangeIter<'a> {
    range: &'a IntRange,
    index: usize,
}

impl <'a> Iterator for IntRangeIter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        match self.range {
            IntRange::Val(vals) if vals.len() > self.index => {
                let v = vals[self.index];
                self.index += 1;
                Some(v)
            },
            IntRange::Inc(start, end, step) => {
                let v = start + step * self.index;
                if v >= *end {
                    return None;
                }
                self.index += 1;
                Some(v)
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn vals_from_str() {
        let r = IntRange::from_str("1,2,3,4,7").unwrap();
        assert_eq!(r.iter().collect::<Vec<_>>(), vec![1, 2, 3, 4, 7]);
    }

    #[test]
    fn range_from_str() {
        let r = IntRange::from_str("0:10:2").unwrap();
        assert_eq!(r.iter().collect::<Vec<_>>(), (0..10).step_by(2).collect::<Vec<_>>());
    }

    #[test]
    fn iter_vals() {
        let r = IntRange::Val((0..10).collect());
        assert_eq!(r.iter().collect::<Vec<_>>(), (0..10).collect::<Vec<_>>());

        let r = IntRange::Val((0..10).step_by(3).collect());
        assert_eq!(r.iter().collect::<Vec<_>>(), (0..10).step_by(3).collect::<Vec<_>>());
    }

    #[test]
    fn iter_range() {
        let r = IntRange::Inc(0, 10, 1);
        assert_eq!(r.iter().collect::<Vec<_>>(), (0..10).collect::<Vec<_>>());

        let r = IntRange::Inc(0, 10, 2);
        assert_eq!(r.iter().collect::<Vec<_>>(), (0..10).step_by(2).collect::<Vec<_>>());
    }
}