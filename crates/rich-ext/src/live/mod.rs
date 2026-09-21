//! Synchronous single-writer coordination for inline terminal regions.
mod region;
use crate::{
    layout::{fit_segments, OverflowPolicy},
    target::RenderTarget,
};
use region::Region;
pub use region::RegionId;
use rich::{
    control::{Control, ControlType},
    protocol::RenderEnvironment,
    Segment,
};
use std::{io::Write, sync::Arc};
#[derive(Debug)]
pub enum LiveError {
    InvalidRegion,
    ExhaustedIds,
    UnsupportedControl,
    Closed,
    Io(std::io::Error),
}
impl std::fmt::Display for LiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "live output: {e}"),
            other => write!(f, "live region error: {other:?}"),
        }
    }
}
impl std::error::Error for LiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(e) = self {
            Some(e)
        } else {
            None
        }
    }
}
impl From<std::io::Error> for LiveError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
pub struct LiveCoordinator<W: Write> {
    writer: W,
    target: RenderTarget,
    owner: Arc<()>,
    next: u64,
    regions: Vec<Region>,
    width: usize,
    height: usize,
    painted: Vec<String>,
    hidden: bool,
    closed: bool,
}
impl<W: Write> LiveCoordinator<W> {
    pub fn new(writer: W, target: RenderTarget) -> Self {
        let c = target.capabilities();
        Self {
            writer,
            target,
            owner: Arc::new(()),
            next: 0,
            regions: Vec::new(),
            width: c.width,
            height: c.height,
            painted: Vec::new(),
            hidden: false,
            closed: false,
        }
    }
    fn check(&self) -> Result<(), LiveError> {
        if self.closed {
            Err(LiveError::Closed)
        } else {
            Ok(())
        }
    }
    pub fn add(&mut self, content: Vec<Segment>) -> Result<RegionId, LiveError> {
        self.check()?;
        validate(&content)?;
        let serial = self.next;
        self.next = self.next.checked_add(1).ok_or(LiveError::ExhaustedIds)?;
        let id = RegionId {
            owner: self.owner.clone(),
            serial,
        };
        self.regions.push(Region {
            id: id.clone(),
            content,
        });
        Ok(id)
    }
    fn index(&self, id: &RegionId) -> Result<usize, LiveError> {
        if !Arc::ptr_eq(&self.owner, &id.owner) {
            return Err(LiveError::InvalidRegion);
        }
        self.regions
            .iter()
            .position(|r| r.id.serial == id.serial)
            .ok_or(LiveError::InvalidRegion)
    }
    pub fn update(&mut self, id: RegionId, content: Vec<Segment>) -> Result<(), LiveError> {
        self.check()?;
        validate(&content)?;
        let index = self.index(&id)?;
        self.regions[index].content = content;
        Ok(())
    }
    pub fn remove(&mut self, id: RegionId) -> Result<(), LiveError> {
        self.check()?;
        let index = self.index(&id)?;
        self.regions.remove(index);
        Ok(())
    }
    pub fn handle(&mut self) -> LiveHandle<'_, W> {
        LiveHandle { live: self }
    }
    fn dynamic(&self) -> bool {
        self.target.capabilities().interactive && self.width > 1 && self.height > 1
    }
    fn control(&mut self, codes: &[ControlType]) -> std::io::Result<()> {
        self.writer
            .write_all(Control::new(codes).as_str().as_bytes())
    }
    fn row_string(&self, mut row: Vec<Segment>) -> String {
        if !self.target.capabilities().hyperlinks {
            for segment in &mut row {
                segment.style = segment.style.as_ref().map(|style| style.update_link(None));
            }
        }
        self.target.console().segments_to_string(&row)
    }
    fn rows(&self, width: usize, height: usize) -> Vec<String> {
        let mut result = Vec::new();
        for region in &self.regions {
            for row in fit_segments(&region.content, width, OverflowPolicy::Crop) {
                if result.len() == height {
                    return result;
                }
                result.push(self.row_string(row));
            }
        }
        result
    }
    fn clear(&mut self) -> std::io::Result<()> {
        let count = self
            .painted
            .len()
            .min(self.height.saturating_sub(1))
            .min(u32::MAX as usize);
        if count == 0 {
            self.painted.clear();
            return Ok(());
        }
        self.control(&[
            ControlType::CarriageReturn,
            ControlType::CursorUp(count as u32),
        ])?;
        for _ in 0..count {
            self.control(&[ControlType::EraseInLine(2)])?;
            self.writer.write_all(b"\n\r")?;
        }
        self.control(&[ControlType::CursorUp(count as u32)])?;
        self.painted.clear();
        Ok(())
    }
    fn paint(&mut self) -> std::io::Result<()> {
        if !self.dynamic() {
            return Ok(());
        }
        let rows = self.rows(self.width - 1, (self.height - 1).min(u32::MAX as usize));
        if rows == self.painted {
            return Ok(());
        }
        if !self.hidden && !rows.is_empty() {
            self.hidden = true;
            self.control(&[ControlType::HideCursor])?;
        }
        if rows.len() == self.painted.len() {
            for (i, row) in rows.iter().enumerate() {
                if row == &self.painted[i] {
                    continue;
                }
                let distance = (rows.len() - i) as u32;
                self.control(&[
                    ControlType::CarriageReturn,
                    ControlType::CursorUp(distance),
                    ControlType::EraseInLine(2),
                ])?;
                self.writer.write_all(row.as_bytes())?;
                self.control(&[
                    ControlType::CarriageReturn,
                    ControlType::CursorDown(distance),
                ])?;
            }
        } else {
            self.clear()?;
            for row in &rows {
                self.control(&[ControlType::CarriageReturn, ControlType::EraseInLine(2)])?;
                self.writer.write_all(row.as_bytes())?;
                self.writer.write_all(b"\n\r")?;
            }
        }
        if rows.is_empty() && self.hidden {
            self.hidden = false;
            self.control(&[ControlType::ShowCursor])?;
        }
        self.painted = rows;
        self.writer.flush()
    }
    fn fail(&mut self, error: std::io::Error) -> LiveError {
        let _ = self.finish();
        LiveError::Io(error)
    }
    pub fn refresh(&mut self) -> Result<(), LiveError> {
        self.check()?;
        self.paint().map_err(|e| self.fail(e))
    }
    pub fn resize(&mut self, width: usize, height: usize) -> Result<(), LiveError> {
        self.check()?;
        self.width = width;
        self.height = height;
        let result = (|| {
            if !self.dynamic() {
                self.painted.clear();
                if self.hidden {
                    self.hidden = false;
                    self.control(&[ControlType::ShowCursor])?;
                }
                return self.writer.flush();
            }
            self.clear()?;
            self.paint()
        })();
        result.map_err(|e| self.fail(e))
    }
    pub fn print(&mut self, content: &[Segment]) -> Result<(), LiveError> {
        self.check()?;
        validate(content)?;
        let result = (|| {
            self.clear()?;
            let interactive = self.target.capabilities().interactive;
            let width = if interactive {
                self.width.saturating_sub(1)
            } else {
                self.width
            };
            for row in fit_segments(content, width, OverflowPolicy::Fold) {
                let text = self.row_string(row);
                self.writer.write_all(text.as_bytes())?;
                self.writer
                    .write_all(if interactive { b"\n\r" } else { b"\n" })?;
            }
            self.paint()
        })();
        result.map_err(|e| self.fail(e))
    }
    pub fn finish(&mut self) -> Result<(), LiveError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut error = None;
        if !self.target.capabilities().interactive {
            for row in self.rows(self.width, self.height) {
                if let Err(e) = writeln!(self.writer, "{row}") {
                    error = Some(e);
                    break;
                }
            }
        } else if let Err(e) = self.clear() {
            error = Some(e);
        }
        if self.hidden {
            self.hidden = false;
            if let Err(e) = self.control(&[ControlType::ShowCursor]) {
                if error.is_none() {
                    error = Some(e);
                }
            }
        }
        if let Err(e) = self.writer.flush() {
            if error.is_none() {
                error = Some(e);
            }
        }
        error.map_or(Ok(()), |e| Err(LiveError::Io(e)))
    }
}
impl<W: Write> Drop for LiveCoordinator<W> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
pub struct LiveHandle<'a, W: Write> {
    live: &'a mut LiveCoordinator<W>,
}
impl<W: Write> LiveHandle<'_, W> {
    pub fn print(&mut self, content: &[Segment]) -> Result<(), LiveError> {
        self.live.print(content)
    }
    pub fn update(&mut self, id: RegionId, content: Vec<Segment>) -> Result<(), LiveError> {
        self.live.update(id, content)
    }
    pub fn refresh(&mut self) -> Result<(), LiveError> {
        self.live.refresh()
    }
}
fn validate(content: &[Segment]) -> Result<(), LiveError> {
    if content.iter().any(|s| {
        s.control
            || s.text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
    }) {
        Err(LiveError::UnsupportedControl)
    } else {
        Ok(())
    }
}
