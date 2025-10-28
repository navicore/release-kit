// Validation logic
// TODO: Implement config validation, file checking, audio metadata detection

pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub info: Vec<String>,
}

pub fn validate_album() -> ValidationReport {
    ValidationReport {
        errors: vec![],
        warnings: vec![],
        info: vec![],
    }
}
