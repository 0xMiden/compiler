use midenc_hir2::{
    dialects::builtin::*, AsCallableSymbolRef, Block, BlockRef, Builder, Felt, Immediate, Listener,
    OpBuilder, Overflow, Region, RegionRef, Report, Signature, SourceSpan, Type,
    UnsafeIntrusiveEntityRef, Usable, ValueRef,
};

use crate::*;

pub struct FunctionBuilder<'f, L: Listener> {
    pub func: &'f mut Function,
    builder: OpBuilder<L>,
}
impl<'f, L: Listener> FunctionBuilder<'f, L> {
    pub fn new(func: &'f mut Function, mut builder: OpBuilder<L>) -> Self {
        let current_block = if func.body().is_empty() {
            func.create_entry_block()
        } else {
            func.last_block()
        };

        builder.set_insertion_point_to_end(current_block);

        Self { func, builder }
    }

    // pub fn at(func: &'f mut Function, ip: midenc_hir2::ProgramPoint) -> Self {
    //     let context = func.as_operation().context_rc();
    //     let mut builder = OpBuilder::new(context);
    //     builder.set_insertion_point(ip);
    //
    //     Self { func, builder }
    // }

    pub fn as_parts_mut(&mut self) -> (&mut Function, &mut OpBuilder<L>) {
        (&mut self.func, &mut self.builder)
    }

    pub fn body_region(&self) -> RegionRef {
        unsafe { RegionRef::from_raw(&*self.func.body()) }
    }

    pub fn entry_block(&self) -> BlockRef {
        self.func.entry_block()
    }

    #[inline]
    pub fn current_block(&self) -> BlockRef {
        self.builder.insertion_block().expect("builder has no insertion point set")
    }

    #[inline]
    pub fn switch_to_block(&mut self, block: BlockRef) {
        self.builder.set_insertion_point_to_end(block);
    }

    pub fn create_block(&mut self) -> BlockRef {
        self.builder.create_block(self.body_region(), None, &[])
    }

    pub fn detach_block(&mut self, mut block: BlockRef) {
        use midenc_hir2::EntityWithParent;

        assert_ne!(
            block,
            self.current_block(),
            "cannot remove block the builder is currently inserting in"
        );
        assert_eq!(
            block.borrow().parent().map(|p| RegionRef::as_ptr(&p)),
            Some(&*self.func.body() as *const Region),
            "cannot detach a block that does not belong to this function"
        );
        let mut body = self.func.body_mut();
        unsafe {
            body.body_mut().cursor_mut_from_ptr(block).remove();
        }
        block.borrow_mut().uses_mut().clear();
        Block::on_removed_from_parent(block, body.as_region_ref());
    }

    pub fn append_block_param(&mut self, block: BlockRef, ty: Type, span: SourceSpan) -> ValueRef {
        self.builder.context().append_block_argument(block, ty, span)
    }

    pub fn ins<'a, 'b: 'a>(&'b mut self) -> DefaultInstBuilder<'a, L> {
        DefaultInstBuilder::new(self.func, &mut self.builder)
    }

    pub fn builder(&self) -> &OpBuilder<L> {
        &self.builder
    }

    pub fn builder_mut(&mut self) -> &mut OpBuilder<L> {
        &mut self.builder
    }
}

pub struct DefaultInstBuilder<'f, L: Listener> {
    func: &'f mut Function,
    builder: &'f mut OpBuilder<L>,
}
impl<'f, L: Listener> DefaultInstBuilder<'f, L> {
    pub(crate) fn new(func: &'f mut Function, builder: &'f mut OpBuilder<L>) -> Self {
        Self { func, builder }
    }
}
impl<L: Listener> InstBuilderBase for DefaultInstBuilder<'_, L> {
    type L = L;

    fn builder(&self) -> &OpBuilder<L> {
        self.builder
    }

    fn builder_mut(&mut self) -> &mut OpBuilder<L> {
        self.builder
    }

    fn builder_parts(&mut self) -> (&mut Function, &mut OpBuilder<Self::L>) {
        (self.func, self.builder)
    }
}

pub trait InstBuilderBase: Sized {
    type L: Listener;
    fn builder(&self) -> &OpBuilder<Self::L>;
    fn builder_mut(&mut self) -> &mut OpBuilder<Self::L>;
    fn builder_parts(&mut self) -> (&mut Function, &mut OpBuilder<Self::L>);
    /// Get a default instruction builder using the dataflow graph and insertion point of the
    /// current builder
    fn ins<'a, 'b: 'a>(&'b mut self) -> DefaultInstBuilder<'a, Self::L> {
        let (func, builder) = self.builder_parts();
        DefaultInstBuilder::new(func, builder)
    }
}

// TODO: remove when the missing instructions are implemented
#[allow(unused_variables, unused_mut)]
pub trait InstBuilder: InstBuilderBase {
    fn assert(
        mut self,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Assert>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Assert, (ValueRef,)>(span);
        op_builder(value)
    }

    fn assert_with_error(
        mut self,
        value: ValueRef,
        code: u32,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Assert>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Assert, (ValueRef, u32)>(span);
        op_builder(value, code)
    }

    fn assertz(
        mut self,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Assertz>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Assertz, (ValueRef,)>(span);
        op_builder(value)
    }

    fn assertz_with_error(
        mut self,
        value: ValueRef,
        code: u32,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Assertz>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Assertz, (ValueRef, u32)>(span);
        op_builder(value, code)
    }

    fn assert_eq(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::AssertEq>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::AssertEq, _>(span);
        op_builder(lhs, rhs)
    }

    fn assert_eq_imm(
        mut self,
        lhs: ValueRef,
        rhs: Immediate,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::AssertEqImm>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::AssertEqImm, _>(span);
        op_builder(lhs, rhs)
    }

    /*
    fn character(self, c: char, span: SourceSpan) -> Value {
        self.i32((c as u32) as i32, span)
    }
    */

    fn i1(mut self, value: bool, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::I1(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn i8(mut self, value: i8, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::I8(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn i16(mut self, value: i16, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::I16(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn i32(mut self, value: i32, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::I32(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn i64(mut self, value: i64, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::I64(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn u8(mut self, value: u8, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::U8(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn u16(mut self, value: u16, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::U16(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn u32(mut self, value: u32, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::U32(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn u64(mut self, value: u64, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::U64(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn f64(mut self, value: f64, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::F64(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn felt(mut self, value: Felt, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::Felt(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn imm(mut self, value: Immediate, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(value).unwrap();
        constant.borrow().result().as_value_ref()
    }

    /// Grow the global heap by `num_pages` pages, in 64kb units.
    ///
    /// Returns the previous size (in pages) of the heap, or -1 if the heap could not be grown.
    fn mem_grow(mut self, num_pages: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::MemGrow, _>(span);
        let op = op_builder(num_pages)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Return the size of the global heap in pages, where each page is 64kb.
    fn mem_size(mut self, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::MemSize, _>(span);
        let op = op_builder()?;
        Ok(op.borrow().result().as_value_ref())
    }

    /*
    /// Get a [GlobalValue] which represents the address of a global variable whose symbol is `name`
    ///
    /// On it's own, this does nothing, you must use the resulting [GlobalValue] with a builder
    /// that expects one as an argument, or use `global_value` to obtain a [Value] from it.
    fn symbol<S: AsRef<str>>(self, name: S, span: SourceSpan) -> GlobalValue {
        self.symbol_relative(name, 0, span)
    }

    /// Same semantics as `symbol`, but applies a constant offset to the address of the given
    /// symbol.
    ///
    /// If the offset is zero, this is equivalent to `symbol`
    fn symbol_relative<S: AsRef<str>>(
        mut self,
        name: S,
        offset: i32,
        span: SourceSpan,
    ) -> GlobalValue {
        self.data_flow_graph_mut().create_global_value(GlobalValueData::Symbol {
            name: Ident::new(Symbol::intern(name.as_ref()), span),
            offset,
        })
    }
    */

    /// Get the address of a global variable whose symbol is `name`
    ///
    /// The type of the pointer produced is given as `ty`. It is up to the caller
    /// to ensure that loading memory from that pointer is valid for the provided
    /// type.
    fn symbol_addr<S: AsRef<str>>(self, name: S, ty: Type, span: SourceSpan) -> ValueRef {
        todo!()
        // self.symbol_relative_addr(name, 0, ty, span)
    }

    /*
    /// Same semantics as `symbol_addr`, but applies a constant offset to the address of the given
    /// symbol.
    ///
    /// If the offset is zero, this is equivalent to `symbol_addr`
    fn symbol_relative_addr<S: AsRef<str>>(
        mut self,
        name: S,
        offset: i32,
        ty: Type,
        span: SourceSpan,
    ) -> Value {
        assert!(ty.is_pointer(), "expected pointer type, got '{}'", &ty);
        let gv = self.data_flow_graph_mut().create_global_value(GlobalValueData::Symbol {
            name: Ident::new(Symbol::intern(name.as_ref()), span),
            offset,
        });
        into_first_result!(self.Global(gv, ty, span))
    }
    */

    /// Loads a value of type `ty` from the global variable whose symbol is `name`.
    ///
    /// NOTE: There is no requirement that the memory contents at the given symbol
    /// contain a valid value of type `ty`. That is left entirely up the caller to
    /// guarantee at a higher level.
    fn load_symbol<S: AsRef<str>>(&self, name: S, ty: Type, span: SourceSpan) -> ValueRef {
        todo!()
        // self.load_symbol_relative(name, ty, 0, span)
    }

    /*
    /// Same semantics as `load_symbol`, but a constant offset is applied to the address before
    /// issuing the load.
    fn load_symbol_relative<S: AsRef<str>>(
        mut self,
        name: S,
        ty: Type,
        offset: i32,
        span: SourceSpan,
    ) -> Value {
        let base = self.data_flow_graph_mut().create_global_value(GlobalValueData::Symbol {
            name: Ident::new(Symbol::intern(name.as_ref()), span),
            offset: 0,
        });
        self.load_global_relative(base, ty, offset, span)
    }

    /// Loads a value of type `ty` from the address represented by `addr`
    ///
    /// NOTE: There is no requirement that the memory contents at the given symbol
    /// contain a valid value of type `ty`. That is left entirely up the caller to
    /// guarantee at a higher level.
    fn load_global(self, addr: GlobalValue, ty: Type, span: SourceSpan) -> Value {
        self.load_global_relative(addr, ty, 0, span)
    }

    /// Same semantics as `load_global_relative`, but a constant offset is applied to the address
    /// before issuing the load.
    fn load_global_relative(
        mut self,
        base: GlobalValue,
        ty: Type,
        offset: i32,
        span: SourceSpan,
    ) -> Value {
        if let GlobalValueData::Load {
            ty: ref base_ty, ..
        } = self.data_flow_graph().global_value(base)
        {
            // If the base global is a load, the target address cannot be computed until runtime,
            // so expand this to the appropriate sequence of instructions to do so in that case
            assert!(base_ty.is_pointer(), "expected global value to have pointer type");
            let base_ty = base_ty.clone();
            let base = self.ins().load_global(base, base_ty.clone(), span);
            let addr = self.ins().ptrtoint(base, Type::U32, span);
            let offset_addr = if offset >= 0 {
                self.ins().add_imm_checked(addr, Immediate::U32(offset as u32), span)
            } else {
                self.ins().sub_imm_checked(addr, Immediate::U32(offset.unsigned_abs()), span)
            };
            let ptr = self.ins().inttoptr(offset_addr, base_ty, span);
            self.load(ptr, span)
        } else {
            // The global address can be computed statically
            let gv = self.data_flow_graph_mut().create_global_value(GlobalValueData::Load {
                base,
                offset,
                ty: ty.clone(),
            });
            into_first_result!(self.Global(gv, ty, span))
        }
    }


    /// Computes an address relative to the pointer produced by `base`, by applying an offset
    /// given by multiplying `offset` * the size in bytes of `unit_ty`.
    ///
    /// The type of the pointer produced is the same as the type of the pointer given by `base`
    ///
    /// This is useful in some scenarios where `load_global_relative` is not, namely when computing
    /// the effective address of an element of an array stored in a global variable.
    fn global_addr_offset(
        mut self,
        base: GlobalValue,
        offset: i32,
        unit_ty: Type,
        span: SourceSpan,
    ) -> Value {
        if let GlobalValueData::Load {
            ty: ref base_ty, ..
        } = self.data_flow_graph().global_value(base)
        {
            // If the base global is a load, the target address cannot be computed until runtime,
            // so expand this to the appropriate sequence of instructions to do so in that case
            assert!(base_ty.is_pointer(), "expected global value to have pointer type");
            let base_ty = base_ty.clone();
            let base = self.ins().load_global(base, base_ty.clone(), span);
            let addr = self.ins().ptrtoint(base, Type::U32, span);
            let unit_size: i32 = unit_ty
                .size_in_bytes()
                .try_into()
                .expect("invalid type: size is larger than 2^32");
            let computed_offset = unit_size * offset;
            let offset_addr = if computed_offset >= 0 {
                self.ins().add_imm_checked(addr, Immediate::U32(offset as u32), span)
            } else {
                self.ins().sub_imm_checked(addr, Immediate::U32(offset.unsigned_abs()), span)
            };
            let ptr = self.ins().inttoptr(offset_addr, base_ty, span);
            self.load(ptr, span)
        } else {
            // The global address can be computed statically
            let gv = self.data_flow_graph_mut().create_global_value(GlobalValueData::IAddImm {
                base,
                offset,
                ty: unit_ty.clone(),
            });
            let ty = self.data_flow_graph().global_type(gv);
            into_first_result!(self.Global(gv, ty, span))
        }
    }

    */

    /// Loads a value of the type pointed to by the given pointer, on to the stack
    ///
    /// NOTE: This function will panic if `ptr` is not a pointer typed value
    fn load(mut self, addr: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Load, _>(span);
        let op = op_builder(addr)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /*
    /// Loads a value from the given temporary (local variable), of the type associated with that
    /// local.
    fn load_local(self, local: LocalId, span: SourceSpan) -> Value {
        let data = Instruction::LocalVar(LocalVarOp {
            op: Opcode::Load,
            local,
            args: ValueList::default(),
        });
        let ty = self.data_flow_graph().local_type(local).clone();
        into_first_result!(self.build(data, Type::Ptr(Box::new(ty)), span))
    }
    */

    /// Stores `value` to the address given by `ptr`
    ///
    /// NOTE: This function will panic if the pointer and pointee types do not match
    fn store(
        mut self,
        ptr: ValueRef,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Store>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Store, _>(span);
        op_builder(ptr, value)
    }

    /*

    /// Stores `value` to the given temporary (local variable).
    ///
    /// NOTE: This function will panic if the type of `value` does not match the type of the local
    /// variable.
    fn store_local(mut self, local: LocalId, value: Value, span: SourceSpan) -> Inst {
        let mut vlist = ValueList::default();
        {
            let dfg = self.data_flow_graph_mut();
            let local_ty = dfg.local_type(local);
            let value_ty = dfg.value_type(value);
            assert_eq!(local_ty, value_ty, "expected value to be a {}, got {}", local_ty, value_ty);
            vlist.push(value, &mut dfg.value_lists);
        }
        let data = Instruction::LocalVar(LocalVarOp {
            op: Opcode::Store,
            local,
            args: vlist,
        });
        self.build(data, Type::Unit, span).0
    }

    */

    /// Writes `count` copies of `value` to memory starting at address `dst`.
    ///
    /// Each copy of `value` will be written to memory starting at the next aligned address from
    /// the previous copy. This instruction will trap if the input address does not meet the
    /// minimum alignment requirements of the type.
    fn memset(
        mut self,
        dst: ValueRef,
        count: ValueRef,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::MemSet>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::MemSet, _>(span);
        op_builder(dst, count, value)
    }

    /// Copies `count` values from the memory at address `src`, to the memory at address `dst`.
    ///
    /// The unit size for `count` is determined by the `src` pointer type, i.e. a pointer to u8
    /// will copy one `count` bytes, a pointer to u16 will copy `count * 2` bytes, and so on.
    ///
    /// NOTE: The source and destination pointer types must match, or this function will panic.
    fn memcpy(
        mut self,
        src: ValueRef,
        dst: ValueRef,
        count: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::MemCpy>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::MemCpy, _>(span);
        op_builder(src, dst, count)
    }

    /// This is a cast operation that permits performing arithmetic on pointer values
    /// by casting a pointer to a specified integral type.
    fn ptrtoint(mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::PtrToInt, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// This is the inverse of `ptrtoint`, used to recover a pointer that was
    /// previously cast to an integer type. It may also be used to cast arbitrary
    /// integer values to pointers.
    ///
    /// In both cases, use of the resulting pointer must not violate the semantics
    /// of the higher level language being represented in Miden IR.
    fn inttoptr(mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::IntToPtr, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /*
    /// This is an intrinsic which derives a new pointer from an existing pointer to an aggregate.
    ///
    /// In short, this represents the common need to calculate a new pointer from an existing
    /// pointer, but without losing provenance of the original pointer. It is specifically
    /// intended for use in obtaining a pointer to an element/field of an array/struct, of the
    /// correct type, given a well typed pointer to the aggregate.
    ///
    /// This function will panic if the pointer is not to an aggregate type
    ///
    /// The new pointer is derived by statically navigating the structure of the pointee type, using
    /// `offsets` to guide the traversal. Initially, the first offset is relative to the original
    /// pointer, where `0` refers to the base/first field of the object. The second offset is then
    /// relative to the base of the object selected by the first offset, and so on. Offsets must
    /// remain in bounds, any attempt to index outside a type's boundaries will result in a
    /// panic.
    fn getelementptr(mut self, ptr: ValueRef, mut indices: &[usize], span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::GetElementPtr>(span);
        op_builder(arg, ty)
    } */

    /// Cast `arg` to a value of type `ty`
    ///
    /// NOTE: This is only supported for integral types currently, and the types must be of the same
    /// size in bytes, i.e. i32 -> u32 or vice versa.
    ///
    /// The intention of bitcasts is to reinterpret a value with different semantics, with no
    /// validation that is typically implied by casting from one type to another.
    fn bitcast(&mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Bitcast, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Cast `arg` to a value of type `ty`
    ///
    /// NOTE: This is only valid for numeric to numeric, or pointer to pointer casts.
    /// For numeric to pointer, or pointer to numeric casts, use `inttoptr` and `ptrtoint`
    /// respectively.
    fn cast(&mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Cast, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Truncates an integral value as necessary to fit in `ty`.
    ///
    /// NOTE: Truncating a value into a larger type has undefined behavior, it is
    /// equivalent to extending a value without doing anything with the new high-order
    /// bits of the resulting value.
    fn trunc(mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Trunc, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Extends an integer into a larger integeral type, by zero-extending the value,
    /// i.e. the new high-order bits of the resulting value will be all zero.
    ///
    /// NOTE: This function will panic if `ty` is smaller than `arg`.
    ///
    /// If `arg` is the same type as `ty`, `arg` is returned as-is
    fn zext(mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Zext, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Extends an integer into a larger integeral type, by sign-extending the value,
    /// i.e. the new high-order bits of the resulting value will all match the sign bit.
    ///
    /// NOTE: This function will panic if `ty` is smaller than `arg`.
    ///
    /// If `arg` is the same type as `ty`, `arg` is returned as-is
    fn sext(mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Sext, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement addition which traps on overflow
    fn add(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Add, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Checked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unchecked two's complement addition. Behavior is undefined if the result overflows.
    fn add_unchecked(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Add, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Unchecked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement addition which wraps around on overflow, e.g. `wrapping_add`
    fn add_wrapping(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Add, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Wrapping)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement addition which wraps around on overflow, but returns a boolean flag that
    /// indicates whether or not the operation overflowed, followed by the wrapped result, e.g.
    /// `overflowing_add` (but with the result types inverted compared to Rust's version).
    fn add_overflowing(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<(ValueRef, ValueRef), Report> {
        let op_builder = self.builder_mut().create::<crate::ops::AddOverflowing, _>(span);
        let op = op_builder(lhs, rhs)?;
        let op = op.borrow();
        let overflowed = op.overflowed().as_value_ref();
        let result = op.result().as_value_ref();
        Ok((overflowed, result))
    }

    /// Two's complement subtraction which traps on under/overflow
    fn sub(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Sub, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Checked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unchecked two's complement subtraction. Behavior is undefined if the result under/overflows.
    fn sub_unchecked(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Sub, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Unchecked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement subtraction which wraps around on under/overflow, e.g. `wrapping_sub`
    fn sub_wrapping(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Sub, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Wrapping)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement subtraction which wraps around on overflow, but returns a boolean flag that
    /// indicates whether or not the operation under/overflowed, followed by the wrapped result,
    /// e.g. `overflowing_sub` (but with the result types inverted compared to Rust's version).
    fn sub_overflowing(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<(ValueRef, ValueRef), Report> {
        let op_builder = self.builder_mut().create::<crate::ops::SubOverflowing, _>(span);
        let op = op_builder(lhs, rhs)?;
        let op = op.borrow();
        let overflowed = op.overflowed().as_value_ref();
        let result = op.result().as_value_ref();
        Ok((overflowed, result))
    }

    /// Two's complement multiplication which traps on overflow
    fn mul(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Mul, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Checked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unchecked two's complement multiplication. Behavior is undefined if the result overflows.
    fn mul_unchecked(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Mul, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Unchecked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement multiplication which wraps around on overflow, e.g. `wrapping_mul`
    fn mul_wrapping(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Mul, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Wrapping)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement multiplication which wraps around on overflow, but returns a boolean flag
    /// that indicates whether or not the operation overflowed, followed by the wrapped result,
    /// e.g. `overflowing_mul` (but with the result types inverted compared to Rust's version).
    fn mul_overflowing(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<(ValueRef, ValueRef), Report> {
        let op_builder = self.builder_mut().create::<crate::ops::MulOverflowing, _>(span);
        let op = op_builder(lhs, rhs)?;
        let op = op.borrow();
        let overflowed = op.overflowed().as_value_ref();
        let result = op.result().as_value_ref();
        Ok((overflowed, result))
    }

    /// Integer division. Traps if `rhs` is zero.
    fn div(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Div, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Integer Euclidean modulo. Traps if `rhs` is zero.
    fn r#mod(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Mod, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Combined integer Euclidean division and modulo. Traps if `rhs` is zero.
    fn divmod(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<(ValueRef, ValueRef), Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Divmod, _>(span);
        let op = op_builder(lhs, rhs)?;
        let op = op.borrow();
        let quotient = op.quotient().as_value_ref();
        let remainder = op.remainder().as_value_ref();
        Ok((quotient, remainder))
    }

    /// Exponentiation
    fn exp(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Exp, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Compute 2^n
    fn pow2(mut self, n: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Pow2, _>(span);
        let op = op_builder(n)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Compute ilog2(n)
    fn ilog2(mut self, n: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Ilog2, _>(span);
        let op = op_builder(n)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Modular inverse
    fn inv(mut self, n: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Inv, _>(span);
        let op = op_builder(n)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unary negation
    fn neg(mut self, n: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Neg, _>(span);
        let op = op_builder(n)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement unary increment by one which traps on overflow
    fn incr(mut self, lhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Incr, _>(span);
        let op = op_builder(lhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Logical AND
    fn and(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::And, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Logical OR
    fn or(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Or, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Logical XOR
    fn xor(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Xor, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Logical NOT
    fn not(mut self, lhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Not, _>(span);
        let op = op_builder(lhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Bitwise AND
    fn band(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Band, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Bitwise OR
    fn bor(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Bor, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Bitwise XOR
    fn bxor(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Bxor, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Bitwise NOT
    fn bnot(mut self, lhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Bnot, _>(span);
        let op = op_builder(lhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn rotl(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Rotl, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn rotr(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Rotr, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn shl(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Shl, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn shr(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Shr, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn popcnt(mut self, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Popcnt, _>(span);
        let op = op_builder(rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn clz(mut self, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Clz, _>(span);
        let op = op_builder(rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn ctz(mut self, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Ctz, _>(span);
        let op = op_builder(rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn clo(mut self, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Clo, _>(span);
        let op = op_builder(rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn cto(mut self, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Cto, _>(span);
        let op = op_builder(rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn eq(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Eq, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn neq(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Neq, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Compares two integers and returns the minimum value
    fn min(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Min, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Compares two integers and returns the maximum value
    fn max(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Max, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn gt(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Gt, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn gt_imm(mut self, cond: ValueRef, zero: Immediate, span: SourceSpan) -> ValueRef {
        todo!()
    }

    fn gte(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Gte, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn lt(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Lt, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn lte(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Lte, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    #[allow(clippy::wrong_self_convention)]
    fn is_odd(mut self, value: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::IsOdd, _>(span);
        let op = op_builder(value)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn exec<C, A>(
        mut self,
        callee: C,
        signature: Signature,
        args: A,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Exec>, Report>
    where
        C: AsCallableSymbolRef,
        A: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<crate::ops::Exec, (C, Signature, A)>(span);
        op_builder(callee, signature, args)
    }

    fn call<C, A>(
        mut self,
        callee: C,
        signature: Signature,
        args: A,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Call>, Report>
    where
        C: AsCallableSymbolRef,
        A: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<crate::ops::Call, (C, Signature, A)>(span);
        op_builder(callee, signature, args)
    }

    fn select(
        mut self,
        cond: ValueRef,
        a: ValueRef,
        b: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Select, _>(span);
        let op = op_builder(cond, a, b)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn br<A>(
        mut self,
        block: BlockRef,
        args: A,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Br>, Report>
    where
        A: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<crate::ops::Br, (_, A)>(span);
        op_builder(block, args)
    }

    fn cond_br<T, F>(
        mut self,
        cond: ValueRef,
        then_dest: BlockRef,
        then_args: T,
        else_dest: BlockRef,
        else_args: F,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::CondBr>, Report>
    where
        T: IntoIterator<Item = ValueRef>,
        F: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<crate::ops::CondBr, (_, _, T, _, F)>(span);
        op_builder(cond, then_dest, then_args, else_dest, else_args)
    }

    // fn switch(self, arg: ValueRef, span: SourceSpan) -> SwitchBuilder<'f, Self> {
    //     todo!()
    //     // require_integer!(self, arg, Type::U32);
    //     // SwitchBuilder::new(self, arg, span)
    // }

    fn ret(
        mut self,
        returning: Option<ValueRef>,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Ret>, Report> {
        let op_builder = self
            .builder_mut()
            .create::<crate::ops::Ret, (<Option<ValueRef> as IntoIterator>::IntoIter,)>(span);
        op_builder(returning)
    }

    fn ret_imm(
        mut self,
        arg: Immediate,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::RetImm>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::RetImm, _>(span);
        op_builder(arg)
    }

    fn unreachable(
        mut self,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Unreachable>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Unreachable, _>(span);
        op_builder()
    }

    /*
    fn inline_asm(
        self,
        args: &[Value],
        results: impl IntoIterator<Item = Type>,
        span: SourceSpan,
    ) -> MasmBuilder<Self> {
        MasmBuilder::new(self, args, results.into_iter().collect(), span)
    }
     */
}

impl<T: InstBuilderBase> InstBuilder for T {}

/*
/// An instruction builder for `switch`, to ensure it is validated during construction
pub struct SwitchBuilder<'f, T: InstBuilder<'f>> {
    builder: T,
    arg: ValueRef,
    span: SourceSpan,
    arms: Vec<SwitchArm>,
    _marker: core::marker::PhantomData<&'f Function>,
}
impl<'f, T: InstBuilder<'f>> SwitchBuilder<'f, T> {
    fn new(builder: T, arg: ValueRef, span: SourceSpan) -> Self {
        Self {
            builder,
            arg,
            span,
            arms: Default::default(),
            _marker: core::marker::PhantomData,
        }
    }

    /// Specify to what block a specific discriminant value should be dispatched
    pub fn case(mut self, discriminant: u32, target: Block, args: &[Value]) -> Self {
        assert_eq!(
            self.arms
                .iter()
                .find(|arm| arm.value == discriminant)
                .map(|arm| arm.successor.destination),
            None,
            "duplicate switch case value '{discriminant}': already matched"
        );
        let mut vlist = ValueList::default();
        {
            let pool = &mut self.builder.data_flow_graph_mut().value_lists;
            vlist.extend(args.iter().copied(), pool);
        }
        let arm = SwitchArm {
            value: discriminant,
            successor: Successor {
                destination: target,
                args: vlist,
            },
        };
        self.arms.push(arm);
        self
    }

    /// Build the `switch` by specifying the fallback destination if none of the arms match
    pub fn or_else(mut self, target: Block, args: &[Value]) -> Inst {
        let mut vlist = ValueList::default();
        {
            let pool = &mut self.builder.data_flow_graph_mut().value_lists;
            vlist.extend(args.iter().copied(), pool);
        }
        let fallback = Successor {
            destination: target,
            args: vlist,
        };
        self.builder.Switch(self.arg, self.arms, fallback, self.span).0
    }
}
 */
