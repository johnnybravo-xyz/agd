//! OSC 7501 program status for terminals such as Rex. Reports use only fixed,
//! bounded fields, and terminals that do not implement the protocol ignore them.

use std::io::{self, IsTerminal, Write};

const CLEAR: &[u8] = b"\x1b]7501;state=clear\x1b\\";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProgramStatus {
    Idle,
    Loading,
    Blocked,
    CaptureDone,
}

impl ProgramStatus {
    fn sequence(self) -> &'static [u8] {
        match self {
            Self::Idle => b"\x1b]7501;state=idle:app=agd\x1b\\",
            Self::Loading => b"\x1b]7501;state=working:app=agd:msg=TG9hZGluZyBhZ2VuZGE=\x1b\\",
            Self::Blocked => {
                b"\x1b]7501;state=blocked:kind=question:app=agd:msg=V2FpdGluZyBmb3IgaW5wdXQ=\x1b\\"
            }
            Self::CaptureDone => b"\x1b]7501;state=done:app=agd:msg=VGFzayBjYXB0dXJlZA==\x1b\\",
        }
    }
}

#[derive(Debug)]
pub(super) struct Reporter {
    enabled: bool,
    last: Option<ProgramStatus>,
}

impl Reporter {
    pub(super) fn terminal() -> Self {
        Self {
            enabled: io::stdout().is_terminal(),
            last: None,
        }
    }

    #[cfg(test)]
    pub(super) fn disabled() -> Self {
        Self {
            enabled: false,
            last: None,
        }
    }

    pub(super) fn report(
        &mut self,
        output: &mut impl Write,
        status: ProgramStatus,
    ) -> io::Result<()> {
        if !self.enabled || self.last == Some(status) {
            return Ok(());
        }
        output.write_all(status.sequence())?;
        output.flush()?;
        self.last = Some(status);
        Ok(())
    }

    pub(super) fn clear(&mut self, output: &mut impl Write) -> io::Result<()> {
        if !self.enabled || self.last.is_none() {
            return Ok(());
        }
        output.write_all(CLEAR)?;
        output.flush()?;
        self.last = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ProgramStatus, Reporter};

    fn reporter() -> Reporter {
        Reporter {
            enabled: true,
            last: None,
        }
    }

    #[test]
    fn reports_bounded_protocol_records_only_when_state_changes() {
        let mut output = Vec::new();
        let mut reporter = reporter();

        reporter
            .report(&mut output, ProgramStatus::Loading)
            .unwrap();
        reporter
            .report(&mut output, ProgramStatus::Loading)
            .unwrap();
        reporter
            .report(&mut output, ProgramStatus::Blocked)
            .unwrap();
        reporter.clear(&mut output).unwrap();

        assert_eq!(
            output,
            concat!(
                "\x1b]7501;state=working:app=agd:msg=TG9hZGluZyBhZ2VuZGE=\x1b\\",
                "\x1b]7501;state=blocked:kind=question:app=agd:msg=V2FpdGluZyBmb3IgaW5wdXQ=\x1b\\",
                "\x1b]7501;state=clear\x1b\\"
            )
            .as_bytes()
        );
    }

    #[test]
    fn a_disabled_reporter_never_writes_terminal_sequences() {
        let mut output = Vec::new();
        let mut reporter = Reporter {
            enabled: false,
            last: None,
        };

        reporter.report(&mut output, ProgramStatus::Idle).unwrap();
        reporter.clear(&mut output).unwrap();

        assert!(output.is_empty());
    }
}
