use crate::{
    calc_result::CalcResult,
    expressions::{
        parser::{ArrayNode, Node},
        token::Error,
        types::CellReferenceIndex,
    },
    model::Model,
};

impl<'a> Model<'a> {
    // ── SUMPRODUCT ────────────────────────────────────────────────────────────

    /// `=SUMPRODUCT(array1, [array2], ...)`
    ///
    /// Multiplies corresponding elements across all arrays and returns the sum.
    /// Non-numeric values (text, booleans, empty) are treated as 0.
    /// All arrays must have the same dimensions; a mismatch yields `#VALUE!`.
    pub(crate) fn fn_sumproduct(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.is_empty() {
            return CalcResult::new_args_number_error(cell);
        }

        // Every array as its numbers, row after row: non-numeric → 0.0,
        // errors propagate. A range is read into them directly; a column of
        // a hundred thousand rows is that many numbers and nothing else.
        let mut arrays: Vec<Vec<f64>> = Vec::with_capacity(args.len());
        let mut size: Option<(usize, usize)> = None;
        let mut same_size = |rows: usize, cols: usize| -> bool {
            *size.get_or_insert((rows, cols)) == (rows, cols)
        };
        let mismatch = || {
            CalcResult::new_error(
                Error::VALUE,
                cell,
                "SUMPRODUCT: arrays must have the same dimensions".to_string(),
            )
        };
        let number = |node: &ArrayNode| -> Result<f64, Error> {
            match node {
                ArrayNode::Number(v) => Ok(*v),
                ArrayNode::Boolean(_) | ArrayNode::String(_) | ArrayNode::Empty => Ok(0.0),
                ArrayNode::Error(e) => Err(e.clone()),
            }
        };
        let error = |error: Error| CalcResult::Error {
            error,
            origin: cell,
            message: String::new(),
        };

        for arg in args {
            let mut numbers: Vec<f64>;
            match self.evaluate_node_in_context(arg, cell) {
                CalcResult::Range { left, right } => {
                    let rows = (right.row - left.row + 1).max(0) as usize;
                    let cols = (right.column - left.column + 1).max(0) as usize;
                    if !same_size(rows, cols) {
                        // Read all the same: what it evaluates stays evaluated
                        self.evaluate_range(left, right);
                        return mismatch();
                    }
                    numbers = Vec::with_capacity(rows * cols);
                    let mut first_error = None;
                    for row in left.row..=right.row {
                        self.read_row(left.sheet, row, left.column, right.column, &mut |node| {
                            match number(&node) {
                                Ok(v) => numbers.push(v),
                                Err(e) => {
                                    first_error.get_or_insert(e);
                                }
                            }
                        });
                    }
                    if let Some(e) = first_error {
                        return error(e);
                    }
                }
                result => {
                    let arr = match self.result_to_array(result, cell) {
                        Ok(a) => a,
                        Err(e) => return e,
                    };
                    let rows = arr.len();
                    let cols = if rows == 0 { 0 } else { arr[0].len() };
                    if !same_size(rows, cols) {
                        return mismatch();
                    }
                    numbers = Vec::with_capacity(rows * cols);
                    for row in &arr {
                        for node in row {
                            match number(node) {
                                Ok(v) => numbers.push(v),
                                Err(e) => return error(e),
                            }
                        }
                    }
                }
            }
            arrays.push(numbers);
        }

        let (rows, cols) = size.unwrap_or((0, 0));
        let mut total = 0.0f64;
        for index in 0..rows * cols {
            let product = arrays.iter().map(|a| a[index]).product::<f64>();
            total += product;
        }

        CalcResult::Number(total)
    }
}
