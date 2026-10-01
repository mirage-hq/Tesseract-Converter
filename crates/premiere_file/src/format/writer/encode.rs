use crate::{format::Result, schema::native::PremiereData};
use serde::Serialize;
use std::fmt;

/// Rewrites carriage returns after escaping so XML parsing preserves them.
struct PreserveCarriageReturns<'a>(&'a mut String);

impl fmt::Write for PreserveCarriageReturns<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        for character in value.chars() {
            if character == '\r' {
                self.0.push_str("&#13;");
            } else {
                self.0.push(character);
            }
        }
        Ok(())
    }
}

pub(super) fn encode(document: &PremiereData) -> Result<String> {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n");
    let mut sink = PreserveCarriageReturns(&mut xml);
    let mut serializer = quick_xml::se::Serializer::new(&mut sink);
    serializer.indent('\t', 1);
    document.serialize(serializer)?;
    xml.push_str("\n\n");
    Ok(xml)
}
