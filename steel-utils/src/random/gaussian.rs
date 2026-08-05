use super::Random;

/// A trait for generating gaussian random numbers using the Marsaglia polar method.
pub trait MarsagliaPolarGaussian: Random {
    /// Gets the stored next gaussian.
    fn stored_next_gaussian(&self) -> Option<f64>;

    /// Sets the stored next gaussian.
    fn set_stored_next_gaussian(&mut self, value: Option<f64>);

    /// Calculates the next gaussian.
    fn calculate_gaussian(&mut self) -> f64 {
        if let Some(gaussian) = self.stored_next_gaussian() {
            self.set_stored_next_gaussian(None);
            gaussian
        } else {
            loop {
                let d = 2.0 * self.next_f64() - 1.0;
                let e = 2.0 * self.next_f64() - 1.0;
                let f = d * d + e * e;

                if f < 1.0 && f != 0.0 {
                    let g = (-2.0 * f.ln() / f).sqrt();
                    self.set_stored_next_gaussian(Some(e * g));
                    return d * g;
                }
            }
        }
    }
}
