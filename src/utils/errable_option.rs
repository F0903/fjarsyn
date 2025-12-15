// #[derive(Debug, Clone, PartialEq, Eq)]
// pub enum ErrableOption<T, E> {
//     Some(T),
//     None,
//     Err(E),
// }

// impl<T, E> ErrableOption<T, E> {
//     pub fn ok(self) -> Option<T> {
//         match self {
//             ErrableOption::Some(value) => Some(value),
//             _ => None,
//         }
//     }

//     pub fn err(self) -> Option<E> {
//         match self {
//             ErrableOption::Err(error) => Some(error),
//             _ => None,
//         }
//     }

//     pub fn as_ref(&self) -> ErrableOption<&T, &E> {
//         match self {
//             ErrableOption::Some(value) => ErrableOption::Some(value),
//             ErrableOption::None => ErrableOption::None,
//             ErrableOption::Err(error) => ErrableOption::Err(error),
//         }
//     }

//     pub fn as_mut(&mut self) -> ErrableOption<&mut T, &mut E> {
//         match self {
//             ErrableOption::Some(value) => ErrableOption::Some(value),
//             ErrableOption::None => ErrableOption::None,
//             ErrableOption::Err(error) => ErrableOption::Err(error),
//         }
//     }

//     pub fn map<U>(self, f: impl FnOnce(T) -> U) -> ErrableOption<U, E> {
//         match self {
//             ErrableOption::Some(value) => ErrableOption::Some(f(value)),
//             ErrableOption::None => ErrableOption::None,
//             ErrableOption::Err(error) => ErrableOption::Err(error),
//         }
//     }
// }

// impl<T, E> ErrableOption<Option<T>, E> {
//     pub fn flatten(self) -> ErrableOption<T, E> {
//         match self {
//             ErrableOption::Some(Some(value)) => ErrableOption::Some(value),
//             ErrableOption::Some(None) => ErrableOption::None,
//             ErrableOption::None => ErrableOption::None,
//             ErrableOption::Err(error) => ErrableOption::Err(error),
//         }
//     }
// }

// impl<T, E> ErrableOption<Result<T, E>, E> {
//     pub fn flatten(self) -> ErrableOption<T, E> {
//         match self {
//             ErrableOption::Some(Ok(value)) => ErrableOption::Some(value),
//             ErrableOption::Some(Err(error)) => ErrableOption::Err(error),
//             ErrableOption::None => ErrableOption::None,
//             ErrableOption::Err(error) => ErrableOption::Err(error),
//         }
//     }
// }
