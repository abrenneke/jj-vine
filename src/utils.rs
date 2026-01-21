/// A trait to get the only element of a collection.
pub trait Only<T> {
    /// Get the only element of the collection.
    /// Returns `None` if the collection is empty or has more than one element.
    fn only(self) -> Option<T>;
}

impl<T> Only<T> for Vec<T> {
    fn only(self) -> Option<T> {
        match self.len() {
            1 => Some(self.into_iter().next().unwrap()),
            _ => None,
        }
    }
}

impl<'a, T> Only<&'a T> for &'a Vec<T> {
    fn only(self) -> Option<&'a T> {
        match self.len() {
            1 => Some(self.first().unwrap()),
            _ => None,
        }
    }
}

impl<'a, T> Only<&'a T> for &'a T
where
    T: AsRef<[T]>,
{
    fn only(self) -> Option<&'a T> {
        match self.as_ref() {
            [x] => Some(x),
            _ => None,
        }
    }
}

impl<'a, T> Only<&'a T> for T
where
    T: IntoIterator<Item = &'a T>,
{
    fn only(self) -> Option<&'a T> {
        let mut iter = self.into_iter();
        match iter.next() {
            Some(x) => {
                if iter.next().is_some() {
                    None
                } else {
                    Some(x)
                }
            }
            None => None,
        }
    }
}
