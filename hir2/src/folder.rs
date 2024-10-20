use alloc::{collections::BTreeMap, rc::Rc};

use rustc_hash::FxHashMap;
use smallvec::{smallvec, SmallVec};

use crate::{
    matchers::Matcher,
    traits::{ConstantLike, Foldable, IsolatedFromAbove},
    AttributeValue, BlockRef, Builder, Context, Dialect, FoldResult, OpFoldResult, OperationRef,
    RegionRef, Rewriter, RewriterImpl, RewriterListener, SourceSpan, Spanned, Type, Value,
    ValueRef,
};

/// Represents a constant value uniqued by dialect, value, and type.
struct UniquedConstant {
    dialect: Rc<dyn Dialect>,
    value: Box<dyn AttributeValue>,
    ty: Type,
}
impl Eq for UniquedConstant {}
impl PartialEq for UniquedConstant {
    fn eq(&self, other: &Self) -> bool {
        use core::hash::{Hash, Hasher};

        let v1_hash = {
            let mut hasher = rustc_hash::FxHasher::default();
            self.value.hash(&mut hasher);
            hasher.finish()
        };
        let v2_hash = {
            let mut hasher = rustc_hash::FxHasher::default();
            other.value.hash(&mut hasher);
            hasher.finish()
        };

        self.dialect.name() == other.dialect.name() && v1_hash == v2_hash && self.ty == other.ty
    }
}
impl UniquedConstant {
    pub fn new(op: &OperationRef, value: Box<dyn AttributeValue>) -> Self {
        let op = op.borrow();
        let dialect = op.dialect();
        let ty = op.results()[0].borrow().ty().clone();

        Self { dialect, value, ty }
    }
}
impl core::hash::Hash for UniquedConstant {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.dialect.name().hash(state);
        self.value.hash(state);
        self.ty.hash(state);
    }
}

/// A map of uniqued constants, to their defining operations
type ConstantMap = FxHashMap<UniquedConstant, OperationRef>;

/// The [OperationFolder] is responsible for orchestrating operation folding, and the effects that
/// folding an operation has on the containing region.
///
/// It handles the following details related to operation folding:
///
/// * Attempting to fold the operation itself
/// * Materializing constants
/// * Uniquing/de-duplicating materialized constants, including moving them up the CFG to ensure
///   that new uses of a uniqued constant are dominated by the constant definition.
/// * Removing folded operations (or cleaning up failed attempts), and notifying any listeners of
///   those actions.
pub struct OperationFolder {
    rewriter: Box<dyn Rewriter>,
    /// A mapping between an insertion region and the constants that have been created within it.
    scopes: BTreeMap<RegionRef, ConstantMap>,
    /// This map tracks all of the dialects that an operation is referenced by; given that multiple
    /// dialects may generate the same constant.
    referenced_dialects: BTreeMap<OperationRef, SmallVec<[Rc<dyn Dialect>; 1]>>,
    /// The location to use for folder-owned constants
    erased_folded_location: SourceSpan,
}
impl OperationFolder {
    pub fn new<L>(context: Rc<Context>, listener: L) -> Self
    where
        L: RewriterListener,
    {
        Self {
            rewriter: Box::new(RewriterImpl::<L>::new(context).with_listener(listener)),
            scopes: Default::default(),
            referenced_dialects: Default::default(),
            erased_folded_location: SourceSpan::UNKNOWN,
        }
    }

    /// Tries to perform folding on `op`, including unifying de-duplicated constants.
    ///
    /// If successful, replaces uses of `op`'s results with the folded results, and returns
    /// a [FoldResult].
    pub fn try_fold(&mut self, mut op: OperationRef) -> FoldResult {
        // If this is a uniqued constant, return failure as we know that it has already been
        // folded.
        if self.is_folder_owned_constant(&op) {
            // Check to see if we should rehoist, i.e. if a non-constant operation was inserted
            // before this one.
            let block = op.borrow().parent().unwrap();
            if block.borrow().front().unwrap() != op
                && !self.is_folder_owned_constant(&op.prev().unwrap())
            {
                let mut op = op.borrow_mut();
                op.move_before(crate::ProgramPoint::Block(block));
                op.set_span(self.erased_folded_location);
            }
            return FoldResult::Failed;
        }

        // Try to fold the operation
        let mut fold_results = SmallVec::default();
        match op.borrow_mut().fold(&mut fold_results) {
            FoldResult::InPlace => {
                // Folding API does not notify listeners, so we need to do so manually
                self.rewriter.notify_operation_modified(op);

                FoldResult::InPlace
            }
            FoldResult::Ok(_) => {
                assert!(
                    !fold_results.is_empty(),
                    "expected non-empty fold results from a successful fold"
                );
                if let FoldResult::Ok(replacements) =
                    self.process_fold_results(op.clone(), &fold_results)
                {
                    // Constant folding succeeded. Replace all of the result values and erase the
                    // operation.
                    self.notify_removal(op.clone());
                    self.rewriter.replace_op_with_values(op, &replacements);
                    FoldResult::Ok(())
                } else {
                    FoldResult::Failed
                }
            }
            failed @ FoldResult::Failed => failed,
        }
    }

    /// Try to process a set of fold results.
    ///
    /// Returns the folded values if successful.
    fn process_fold_results(
        &mut self,
        op: OperationRef,
        fold_results: &[OpFoldResult],
    ) -> FoldResult<SmallVec<[ValueRef; 2]>> {
        let borrowed_op = op.borrow();
        assert_eq!(fold_results.len(), borrowed_op.num_results());

        // Create a builder to insert new operations into the entry block of the insertion region
        let insert_region = get_insertion_region(borrowed_op.parent().unwrap());
        let entry = insert_region.borrow().entry_block_ref().unwrap();
        self.rewriter.set_insertion_point_to_start(entry);

        // Create the result constants and replace the results.
        let dialect = borrowed_op.dialect();
        let mut out = SmallVec::default();
        for (op_result, fold_result) in borrowed_op.results().iter().zip(fold_results) {
            match fold_result {
                // Check if the result was an SSA value.
                OpFoldResult::Value(value) => {
                    out.push(value.clone());
                    continue;
                }
                // Check to see if there is a canonicalized version of this constant.
                OpFoldResult::Attribute(attr_repl) => {
                    if let Some(mut const_op) = self.try_get_or_create_constant(
                        insert_region.clone(),
                        dialect.clone(),
                        attr_repl.clone_value(),
                        op_result.borrow().ty().clone(),
                        self.erased_folded_location,
                    ) {
                        // Ensure that this constant dominates the operation we are replacing.
                        //
                        // This may not automatically happen if the operation being folded was
                        // inserted before the constant within the insertion block.
                        let op_block = borrowed_op.parent().unwrap();
                        if op_block == const_op.borrow().parent().unwrap()
                            && op_block.borrow().front().unwrap() != const_op
                        {
                            const_op.borrow_mut().move_before(crate::ProgramPoint::Block(op_block));
                        }
                        out.push(const_op.borrow().get_result(0).borrow().as_value_ref());
                        continue;
                    }

                    // If materialization fails, clean up any operations generated for the previous
                    // results and return failure.
                    let inserted_before = self.rewriter.insertion_point().unwrap().op();
                    if let Some(inserted_before) = inserted_before {
                        while let Some(inserted_op) = inserted_before.prev() {
                            self.notify_removal(inserted_op.clone());
                            self.rewriter.erase_op(inserted_op);
                        }
                    }

                    return FoldResult::Failed;
                }
            }
        }

        FoldResult::Ok(out)
    }

    /// Notifies that the given constant `op` should be removed from this folder's internal
    /// bookkeeping.
    ///
    /// NOTE: This method must be called if a constant op is to be deleted externally to this
    /// folder. `op` must be constant.
    pub fn notify_removal(&mut self, op: OperationRef) {
        // Check to see if this operation is uniqued within the folder.
        let Some(referenced_dialects) = self.referenced_dialects.get_mut(&op) else {
            return;
        };

        let borrowed_op = op.borrow();

        // Get the constant value for this operation, this is the value that was used to unique
        // the operation internally.
        let value = crate::matchers::constant().matches(&borrowed_op).unwrap();

        // Get the constant map that this operation was uniqued in.
        let insert_region = get_insertion_region(borrowed_op.parent().unwrap());
        let uniqued_constants = self.scopes.get_mut(&insert_region).unwrap();

        // Erase all of the references to this operation.
        let ty = borrowed_op.results()[0].borrow().ty().clone();
        for dialect in referenced_dialects.drain(..) {
            let uniqued_constant = UniquedConstant {
                dialect,
                value: value.clone_value(),
                ty: ty.clone(),
            };
            uniqued_constants.remove(&uniqued_constant);
        }
    }

    /// CLear out any constants cached inside the folder.
    pub fn clear(&mut self) {
        self.scopes.clear();
        self.referenced_dialects.clear();
    }

    /// Tries to fold a pre-existing constant operation.
    ///
    /// `value` represents the value of the constant, and can be optionally passed if the value is
    /// already known (e.g. if the constant was discovered by a pattern match). This is purely an
    /// optimization opportunity for callers that already know the value of the constant.
    ///
    /// Returns `false` if an existing constant for `op` already exists in the folder, in which case
    /// `op` is replaced and erased. Otherwise, returns `true` and `op` is inserted into the folder
    /// and hoisted if necessary.
    pub fn insert_known_constant(
        &mut self,
        mut op: OperationRef,
        value: Option<Box<dyn AttributeValue>>,
    ) -> bool {
        let block = op.borrow().parent().unwrap();

        // If this is a constant we uniqued, we don't need to insert, but we can check to see if
        // we should rehoist it.
        if self.is_folder_owned_constant(&op) {
            if block.borrow().front().unwrap() != op
                && !self.is_folder_owned_constant(&op.prev().unwrap())
            {
                let mut op = op.borrow_mut();
                op.move_before(crate::ProgramPoint::Block(block));
                op.set_span(self.erased_folded_location);
            }
            return true;
        }

        // Get the constant value of the op if necessary.
        let value = value.unwrap_or_else(|| {
            crate::matchers::constant()
                .matches(&op.borrow())
                .expect("expected `op` to be a constant")
        });

        // Check for an existing constant operation for the attribute value.
        let insert_region = get_insertion_region(block.clone());
        let uniqued_constants = self.scopes.entry(insert_region.clone()).or_default();
        let uniqued_constant = UniquedConstant::new(&op, value);
        let mut is_new = false;
        let mut folder_const_op = uniqued_constants
            .entry(uniqued_constant)
            .or_insert_with(|| {
                is_new = true;
                op.clone()
            })
            .clone();

        // If there is an existing constant, replace `op`
        if !is_new {
            self.notify_removal(op.clone());
            self.rewriter.replace_op(op, folder_const_op.clone());
            folder_const_op.borrow_mut().set_span(self.erased_folded_location);
            return false;
        }

        // Otherwise, we insert `op`. If `op` is in the insertion block and is either already at the
        // front of the block, or the previous operation is already a constant we uniqued (i.e. one
        // we inserted), then we don't need to do anything. Otherwise, we move the constant to the
        // insertion block.
        let insert_block = insert_region.borrow().entry_block_ref().unwrap();
        if block != insert_block
            || (insert_block.borrow().front().unwrap() != op
                && !self.is_folder_owned_constant(&op.prev().unwrap()))
        {
            let mut op = op.borrow_mut();
            op.move_before(crate::ProgramPoint::Block(insert_block));
            op.set_span(self.erased_folded_location);
        }

        let referenced_dialects = self.referenced_dialects.entry(op.clone()).or_default();
        let dialect = op.borrow().dialect();
        let dialect_name = dialect.name();
        if !referenced_dialects.iter().any(|d| d.name() == dialect_name) {
            referenced_dialects.push(dialect);
        }

        true
    }

    /// Get or create a constant for use in the specified block.
    ///
    /// The constant may be created in a parent block. On success, this returns the result of the
    /// constant operation, or `None` otherwise.
    pub fn get_or_create_constant(
        &mut self,
        block: BlockRef,
        dialect: Rc<dyn Dialect>,
        value: Box<dyn AttributeValue>,
        ty: Type,
    ) -> Option<ValueRef> {
        // Find an insertion point for the constant.
        let insert_region = get_insertion_region(block.clone());
        let entry = insert_region.borrow().entry_block_ref().unwrap();
        self.rewriter.set_insertion_point_to_start(entry);

        // Get the constant map for the insertion region of this operation.
        // Use erased location since the op is being built at the front of the block.
        let const_op = self.try_get_or_create_constant(
            insert_region,
            dialect,
            value,
            ty,
            self.erased_folded_location,
        )?;
        Some(const_op.borrow().results()[0].borrow().as_value_ref())
    }

    /// Try to get or create a new constant entry.
    ///
    /// On success, this returns the constant operation, `None` otherwise
    fn try_get_or_create_constant(
        &mut self,
        insert_region: RegionRef,
        dialect: Rc<dyn Dialect>,
        value: Box<dyn AttributeValue>,
        ty: Type,
        span: SourceSpan,
    ) -> Option<OperationRef> {
        let uniqued_constants = self.scopes.entry(insert_region).or_default();
        let uniqued_constant = UniquedConstant {
            dialect: dialect.clone(),
            value,
            ty,
        };
        if let Some(mut const_op) = uniqued_constants.get(&uniqued_constant).cloned() {
            {
                let mut const_op = const_op.borrow_mut();
                if const_op.span() != span {
                    const_op.set_span(span);
                }
            }
            return Some(const_op);
        }

        // If one doesn't exist, try to materialize one.
        let const_op = materialize_constant(
            self.rewriter.as_mut(),
            dialect.clone(),
            uniqued_constant.value.clone_value(),
            &uniqued_constant.ty,
            span,
        )?;

        // Check to see if the generated constant is in the expected dialect.
        let new_dialect = const_op.borrow().dialect();
        if new_dialect.name() == dialect.name() {
            self.referenced_dialects.entry(const_op.clone()).or_default().push(new_dialect);
            return Some(const_op);
        }

        // If it isn't, then we also need to make sure that the mapping for the new dialect is valid
        let new_uniqued_constant = UniquedConstant {
            dialect: new_dialect.clone(),
            value: uniqued_constant.value.clone_value(),
            ty: uniqued_constant.ty.clone(),
        };
        let maybe_existing_op = uniqued_constants.get(&new_uniqued_constant).cloned();
        uniqued_constants.insert(
            uniqued_constant,
            maybe_existing_op.clone().unwrap_or_else(|| const_op.clone()),
        );
        if let Some(mut existing_op) = maybe_existing_op {
            self.notify_removal(const_op.clone());
            self.rewriter.erase_op(const_op);
            self.referenced_dialects
                .get_mut(&existing_op)
                .unwrap()
                .push(new_uniqued_constant.dialect.clone());
            let mut existing = existing_op.borrow_mut();
            if existing.span() != span {
                existing.set_span(span);
            }
            Some(existing_op)
        } else {
            self.referenced_dialects
                .insert(const_op.clone(), smallvec![dialect, new_dialect]);
            uniqued_constants.insert(new_uniqued_constant, const_op.clone());
            Some(const_op)
        }
    }

    /// Returns true if the given operation is an already folded constant that is owned by this
    /// folder.
    #[inline(always)]
    fn is_folder_owned_constant(&self, op: &OperationRef) -> bool {
        self.referenced_dialects.contains_key(op)
    }
}

/// Materialize a constant for a given attribute and type.
///
/// Returns a constant operation if successful, otherwise `None`
fn materialize_constant(
    builder: &mut dyn Builder,
    dialect: Rc<dyn Dialect>,
    value: Box<dyn AttributeValue>,
    ty: &Type,
    span: SourceSpan,
) -> Option<OperationRef> {
    let ip = builder.insertion_point().cloned();

    // Ask the dialect to materialize a constant operation for this value.
    let const_op = dialect.materialize_constant(builder, value, ty, span)?;
    assert_eq!(ip.as_ref(), builder.insertion_point());
    assert!(const_op.borrow().implements::<dyn ConstantLike>());
    Some(const_op)
}

/// Given the containing block of an operation, find the parent region that folded constants should
/// be inserted into.
fn get_insertion_region(insertion_block: BlockRef) -> RegionRef {
    use crate::EntityWithId;

    let mut insertion_block = Some(insertion_block);
    while let Some(block) = insertion_block.take() {
        let parent_region = block.borrow().parent().unwrap_or_else(|| {
            panic!("expected block {} to be attached to a region", block.borrow().id())
        });
        // Insert in this region for any of the following scenarios:
        //
        // * The parent is known to be isolated from above
        // * The parent is a top-level operation
        let parent_op = parent_region
            .borrow()
            .parent()
            .expect("expected region to be attached to an operation");
        let parent = parent_op.borrow();
        let parent_block = parent.parent();
        if parent.implements::<dyn IsolatedFromAbove>() || parent_block.is_none() {
            return parent_region;
        }

        insertion_block = parent_block;
    }

    unreachable!("expected valid insertion region")
}
