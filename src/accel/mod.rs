//! # accel
//!
//! Traits and structs for representing accelerometers.

use super::{Rotation,Orientator,SENSITIVITY};

use std::ops::{Add,Div,Sub,Mul,AddAssign};
use std::fmt::{Display, Formatter};
use std::fmt::Result as FmtResult;

#[cfg(feature = "fsaccel")]
pub mod fsaccel;
pub use self::fsaccel::FsAccelerometer as FsAccel;


/// Describes an acceleration vector.
#[derive(Default, Debug, Clone, Copy)]
pub struct AccelerationVector<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}

impl<T> Display for AccelerationVector<T>
    where T: Display
{
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "x: {} | y: {} | z: {}", self.x, self.y, self.z)
    }
}

impl<T> Add<AccelerationVector<T>> for AccelerationVector<T> where T: Add<T> + Default + Clone + Copy + PartialOrd {
    type Output = AccelerationVector<<T as Add>::Output>;
    fn add(self, other: AccelerationVector<T>) -> AccelerationVector<<T as Add>::Output> {
        AccelerationVector::<<T as Add>::Output> {
            x: { self.x + other.x },
            y: { self.y + other.y },
            z: { self.z + other.z },
        }
    }
}

impl<T> Sub<AccelerationVector<T>> for AccelerationVector<T> where T: Sub<Output=T> + Default + Clone + Copy + PartialOrd {
    type Output = AccelerationVector<T>;
    fn sub(self, other: AccelerationVector<T>) -> AccelerationVector<T> {
        AccelerationVector::<T> {
            x: { self.x - other.x },
            y: { self.y - other.y },
            z: { self.z - other.z }
        }
    }
}

impl<T> AddAssign<AccelerationVector<T>> for AccelerationVector<T> where T: AddAssign<T> + Default + Clone + Copy + PartialOrd {
    fn add_assign(&mut self, other: AccelerationVector<T>) {
        self.x += other.x;
        self.y += other.y;
        self.z += other.z;
    }
}

impl<T,U> Mul<U> for AccelerationVector<T> where U: Into<f64>, T: Mul<f64> + Default + Clone + Copy + PartialOrd {
    type Output = AccelerationVector<<T as Mul<f64>>::Output>;
    fn mul(self, other: U) -> AccelerationVector<<T as Mul<f64>>::Output> {
        let m = other.into();
        AccelerationVector::<<T as Mul<f64>>::Output> {
            x: { self.x * m },
            y: { self.y * m },
            z: { self.z * m },
        }
    }
}

impl<T,U> Div<U> for AccelerationVector<T> where U: Into<f64>, T: Div<f64> + Default + Clone + Copy + PartialOrd {
    type Output = AccelerationVector<<T as Div<f64>>::Output>;
    fn div(self, other: U) -> AccelerationVector<<T as Div<f64>>::Output> {
        let d = other.into();
        AccelerationVector::<<T as Div<f64>>::Output> {
            x: { self.x / d },
            y: { self.y / d },
            z: { self.z / d },
        }
    }
}


/// Trait for an accelerometer
pub trait Accelerometer {
    /// Returns the scaled output of an accelerometer, preferably in m/s^2.
    /// Up, right, and towards-the-observer should be positive.
    fn read(&mut self) -> AccelerationVector<f64>;

    /// Returns the raw output of an accelerometer.
    /// Up, right, and towards-the-observer should be positive.
    /// i32 for easy conversion to f64
    fn read_raw(&mut self) -> AccelerationVector<i32>;

    /// Returns the scale between raw integers and m/s^2.
    fn get_scale(&self) -> f64;
}

impl<T: Accelerometer> Orientator for T {
    fn orientation(&mut self) -> Option<Rotation> {
        let acc = self.read();
        if (acc.x.abs() - acc.y.abs()).abs() > acc.z.abs() / *SENSITIVITY + 1.4715 {
            if acc.x.abs() > acc.y.abs() {
                if acc.x < 0.0 {
                    trace!("rot: {}; accel: {}", Rotation::Right, acc);
                    Some(Rotation::Right)
                } else {
                    trace!("rot: {}; accel: {}", Rotation::Left, acc);
                    Some(Rotation::Left)
                }
            } else {
                if acc.y < 0.0 {
                    trace!("rot: {}; accel: {}", Rotation::Normal, acc);
                    Some(Rotation::Normal)
                } else {
                    trace!("rot: {}; accel: {}", Rotation::Inverted, acc);
                    Some(Rotation::Inverted)
                }
            }
        } else {
            trace!("rot: {}; accel: {}", "None (dxy too low)", acc);
            None
        }
    }
}


/// Trait for accelerometer with low-pass filtering
#[derive(Default, Debug, Clone, Copy)]
pub struct FilteredAccelerometer<T> {
    accel: T,
    mult: f64,
    current: AccelerationVector<f64>
}

impl<T: Accelerometer> FilteredAccelerometer<T> {
    pub fn new(mut accel: T, mult: f64) -> FilteredAccelerometer<T> {
        let ival = accel.read();
        FilteredAccelerometer::<T> {
            accel: accel,
            mult: mult,
            current: ival,
        }
    }

    pub fn update(&mut self) {
        self.current += (self.accel.read() - self.current) * self.mult;
    }

    pub fn raw_estimate(&self) -> AccelerationVector<i32> {
        let av = self.current / self.accel.get_scale();
        AccelerationVector::<i32> {
            x: av.x.round() as i32,
            y: av.y.round() as i32,
            z: av.z.round() as i32
        }
    }
}

impl<T: Accelerometer> Accelerometer for FilteredAccelerometer<T> {
    fn read(&mut self) -> AccelerationVector<f64> {
        self.update();
        return self.current
    }

    fn read_raw(&mut self) -> AccelerationVector<i32> {
        self.update();
        return self.raw_estimate()
    }

    fn get_scale(&self) -> f64 {
        self.accel.get_scale()
    }
}

impl<'l, T: Accelerometer> Accelerometer for &'l mut FilteredAccelerometer<T> {
    fn read(&mut self) -> AccelerationVector<f64> {
        self.update();
        return self.current
    }

    fn read_raw(&mut self) -> AccelerationVector<i32> {
        self.update();
        return self.raw_estimate()
    }

    fn get_scale(&self) -> f64 {
        self.accel.get_scale()
    }
}

