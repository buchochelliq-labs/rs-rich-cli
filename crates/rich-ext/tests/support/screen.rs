use std::{
    cell::RefCell,
    io::{self, Write},
    rc::Rc,
};
#[derive(Debug)]
pub struct Screen {
    pub width: usize,
    pub height: usize,
    pub visible: bool,
    pub wraps: usize,
    x: usize,
    y: usize,
    cells: Vec<Vec<String>>,
}
impl Screen {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            visible: true,
            wraps: 0,
            x: 0,
            y: height - 1,
            cells: vec![vec![String::from(" "); width]; height],
        }
    }
    pub fn resize(&mut self, width: usize, height: usize) {
        let height = height.max(1);
        let width = width.max(1);
        if self.cells.len() > height {
            self.cells.drain(..self.cells.len() - height);
        }
        for row in &mut self.cells {
            row.resize(width, " ".into());
        }
        self.cells.resize(height, vec![" ".into(); width]);
        self.width = width;
        self.height = height;
        self.y = self.y.min(height - 1);
        self.x = self.x.min(width - 1);
    }
    pub fn lines(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|r| r.concat().trim_end().to_owned())
            .collect()
    }
    fn newline(&mut self) {
        self.y += 1;
        if self.y == self.height {
            self.cells.remove(0);
            self.cells.push(vec![" ".into(); self.width]);
            self.y -= 1;
        }
    }
    pub fn feed(&mut self, text: &str) {
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => {
                    assert_eq!(chars.next(), Some('['));
                    let mut args = String::new();
                    let command = loop {
                        let c = chars.next().expect("complete CSI");
                        if ('@'..='~').contains(&c) {
                            break c;
                        }
                        args.push(c);
                    };
                    let n = args.parse::<usize>().unwrap_or(1);
                    match command {
                        'A' => self.y = self.y.saturating_sub(n),
                        'B' => self.y = (self.y + n).min(self.height - 1),
                        'K' => {
                            assert_eq!(n, 2);
                            self.cells[self.y].fill(" ".into());
                        }
                        'm' => {}
                        'h' | 'l' => {
                            assert_eq!(args, "?25");
                            self.visible = command == 'h';
                        }
                        other => panic!("unsupported CSI {args}{other}"),
                    }
                }
                '\r' => self.x = 0,
                '\n' => self.newline(),
                c => {
                    let w = rich::cells::char_cell_width(c);
                    if w == 0 {
                        if self.x > 0 {
                            self.cells[self.y][self.x - 1].push(c);
                        }
                        continue;
                    }
                    if self.x + w > self.width {
                        self.wraps += 1;
                        self.x = 0;
                        self.newline();
                    }
                    self.cells[self.y][self.x] = c.to_string();
                    if w == 2 {
                        self.cells[self.y][self.x + 1] = String::new();
                    }
                    self.x += w;
                }
            }
        }
    }
}
#[derive(Clone)]
pub struct Writer(pub Rc<RefCell<Screen>>);
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .borrow_mut()
            .feed(std::str::from_utf8(bytes).unwrap());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
