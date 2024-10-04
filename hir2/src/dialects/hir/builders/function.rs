use self::traits::AsCallableSymbolRef;
use crate::*;

pub struct FunctionBuilder<'f> {
    pub func: &'f mut Function,
    builder: OpBuilder,
}
impl<'f> FunctionBuilder<'f> {
    pub fn new(func: &'f mut Function) -> Self {
        let context = func.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);
        builder.set_insertion_point_to_end(func.last_block());

        Self { func, builder }
    }

    pub fn at(func: &'f mut Function, ip: InsertionPoint) -> Self {
        let context = func.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);
        builder.set_insertion_point(ip);

        Self { func, builder }
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
        self.builder.create_block(self.body_region(), None, None)
    }

    pub fn detach_block(&mut self, mut block: BlockRef) {
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
            body.body_mut().cursor_mut_from_ptr(block.clone()).remove();
        }
        block.borrow_mut().uses_mut().clear();
    }

    pub fn append_block_param(&mut self, block: BlockRef, ty: Type, span: SourceSpan) -> ValueRef {
        self.builder.context().append_block_argument(block, ty, span)
    }

    pub fn ins<'a, 'b: 'a>(&'b mut self) -> DefaultInstBuilder<'a> {
        DefaultInstBuilder::new(self.func, &mut self.builder)
    }
}

pub struct DefaultInstBuilder<'f> {
    func: &'f mut Function,
    builder: &'f mut OpBuilder,
}
impl<'f> DefaultInstBuilder<'f> {
    pub(crate) fn new(func: &'f mut Function, builder: &'f mut OpBuilder) -> Self {
        Self { func, builder }
    }
}
impl<'f> InstBuilderBase<'f> for DefaultInstBuilder<'f> {
    fn builder_parts(&mut self) -> (&mut Function, &mut OpBuilder) {
        (self.func, self.builder)
    }

    fn builder(&self) -> &OpBuilder {
        self.builder
    }

    fn builder_mut(&mut self) -> &mut OpBuilder {
        self.builder
    }
}

pub trait InstBuilderBase<'f>: Sized {
    fn builder(&self) -> &OpBuilder;
    fn builder_mut(&mut self) -> &mut OpBuilder;
    fn builder_parts(&mut self) -> (&mut Function, &mut OpBuilder);
    /// Get a default instruction builder using the dataflow graph and insertion point of the
    /// current builder
    fn ins<'a, 'b: 'a>(&'b mut self) -> DefaultInstBuilder<'a> {
        let (func, builder) = self.builder_parts();
        DefaultInstBuilder::new(func, builder)
    }
}

pub trait InstBuilder<'f>: InstBuilderBase<'f> {
    fn assert(
        mut self,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::Assert>, Report> {
        let op_builder =
            self.builder_mut().create::<crate::dialects::hir::Assert, (ValueRef,)>(span);
        op_builder(value)
    }

    fn assert_with_error(
        mut self,
        value: ValueRef,
        code: u32,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::Assert>, Report> {
        let op_builder =
            self.builder_mut().create::<crate::dialects::hir::Assert, (ValueRef, u32)>(span);
        op_builder(value, code)
    }

    fn assertz(
        mut self,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::Assertz>, Report> {
        let op_builder =
            self.builder_mut().create::<crate::dialects::hir::Assertz, (ValueRef,)>(span);
        op_builder(value)
    }

    fn assertz_with_error(
        mut self,
        value: ValueRef,
        code: u32,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::Assertz>, Report> {
        let op_builder = self
            .builder_mut()
            .create::<crate::dialects::hir::Assertz, (ValueRef, u32)>(span);
        op_builder(value, code)
    }

    fn assert_eq(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::AssertEq>, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::AssertEq, _>(span);
        op_builder(lhs, rhs)
    }

    fn assert_eq_imm(
        mut self,
        lhs: ValueRef,
        rhs: Immediate,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::AssertEqImm>, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::AssertEqImm, _>(span);
        op_builder(lhs, rhs)
    }

    //signed_integer_literal!(1, bool);
    //integer_literal!(8);
    //integer_literal!(16);
    //integer_literal!(32);
    //integer_literal!(64);
    //integer_literal!(128);

    /*
    fn felt(self, i: Felt, span: SourceSpan) -> Value {
        into_first_result!(self.UnaryImm(Opcode::ImmFelt, Type::Felt, Immediate::Felt(i), span))
    }

    fn f64(self, f: f64, span: SourceSpan) -> Value {
        into_first_result!(self.UnaryImm(Opcode::ImmF64, Type::F64, Immediate::F64(f), span))
    }

    fn character(self, c: char, span: SourceSpan) -> Value {
        self.i32((c as u32) as i32, span)
    }
    */

    /// Grow the global heap by `num_pages` pages, in 64kb units.
    ///
    /// Returns the previous size (in pages) of the heap, or -1 if the heap could not be grown.
    fn mem_grow(mut self, num_pages: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::MemGrow, _>(span);
        let op = op_builder(num_pages)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Return the size of the global heap in pages, where each page is 64kb.
    fn mem_size(mut self, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::MemSize, _>(span);
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

    /// Get the address of a global variable whose symbol is `name`
    ///
    /// The type of the pointer produced is given as `ty`. It is up to the caller
    /// to ensure that loading memory from that pointer is valid for the provided
    /// type.
    fn symbol_addr<S: AsRef<str>>(self, name: S, ty: Type, span: SourceSpan) -> Value {
        self.symbol_relative_addr(name, 0, ty, span)
    }

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

    /// Loads a value of type `ty` from the global variable whose symbol is `name`.
    ///
    /// NOTE: There is no requirement that the memory contents at the given symbol
    /// contain a valid value of type `ty`. That is left entirely up the caller to
    /// guarantee at a higher level.
    fn load_symbol<S: AsRef<str>>(self, name: S, ty: Type, span: SourceSpan) -> Value {
        self.load_symbol_relative(name, ty, 0, span)
    }

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

    /// Loads a value of the type pointed to by the given pointer, on to the stack
    ///
    /// NOTE: This function will panic if `ptr` is not a pointer typed value
    fn load(self, addr: Value, span: SourceSpan) -> Value {
        let ty = require_pointee!(self, addr).clone();
        let data = Instruction::Load(LoadOp {
            op: Opcode::Load,
            addr,
            ty: ty.clone(),
        });
        into_first_result!(self.build(data, Type::Ptr(Box::new(ty)), span))
    }

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

    /// Stores `value` to the address given by `ptr`
    ///
    /// NOTE: This function will panic if the pointer and pointee types do not match
    fn store(mut self, ptr: Value, value: Value, span: SourceSpan) -> Inst {
        let pointee_ty = require_pointee!(self, ptr);
        let value_ty = self.data_flow_graph().value_type(value);
        assert_eq!(pointee_ty, value_ty, "expected value to be a {}, got {}", pointee_ty, value_ty);
        let mut vlist = ValueList::default();
        {
            let dfg = self.data_flow_graph_mut();
            vlist.extend([ptr, value], &mut dfg.value_lists);
        }
        self.PrimOp(Opcode::Store, Type::Unit, vlist, span).0
    }

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
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::MemSet>, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::MemSet, _>(span);
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
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::MemCpy>, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::MemCpy, _>(span);
        op_builder(src, dst, count)
    }

    /// This is a cast operation that permits performing arithmetic on pointer values
    /// by casting a pointer to a specified integral type.
    fn ptrtoint(mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::PtrToInt, _>(span);
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
        let op_builder = self.builder_mut().create::<crate::dialects::hir::IntToPtr, _>(span);
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
        let op_builder = self.builder_mut().create::<crate::dialects::hir::GetElementPtr>(span);
        op_builder(arg, ty)
    } */

    /// Cast `arg` to a value of type `ty`
    ///
    /// NOTE: This is only supported for integral types currently, and the types must be of the same
    /// size in bytes, i.e. i32 -> u32 or vice versa.
    ///
    /// The intention of bitcasts is to reinterpret a value with different semantics, with no
    /// validation that is typically implied by casting from one type to another.
    fn bitcast(mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Bitcast, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Cast `arg` to a value of type `ty`
    ///
    /// NOTE: This is only valid for numeric to numeric, or pointer to pointer casts.
    /// For numeric to pointer, or pointer to numeric casts, use `inttoptr` and `ptrtoint`
    /// respectively.
    fn cast(mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Cast, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Truncates an integral value as necessary to fit in `ty`.
    ///
    /// NOTE: Truncating a value into a larger type has undefined behavior, it is
    /// equivalent to extending a value without doing anything with the new high-order
    /// bits of the resulting value.
    fn trunc(mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Trunc, _>(span);
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
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Zext, _>(span);
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
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Sext, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /*
    binary_int_op_with_overflow!(add, Opcode::Add);
    binary_int_op_with_overflow!(sub, Opcode::Sub);
    binary_int_op_with_overflow!(mul, Opcode::Mul);
    checked_binary_int_op!(div, Opcode::Div);
    binary_int_op!(min, Opcode::Min);
    binary_int_op!(max, Opcode::Max);
    checked_binary_int_op!(r#mod, Opcode::Mod);
    checked_binary_int_op!(divmod, Opcode::DivMod);
    binary_int_op!(exp, Opcode::Exp);
    binary_boolean_op!(and, Opcode::And);
    binary_int_op!(band, Opcode::Band);
    binary_boolean_op!(or, Opcode::Or);
    binary_int_op!(bor, Opcode::Bor);
    binary_boolean_op!(xor, Opcode::Xor);
    binary_int_op!(bxor, Opcode::Bxor);
    unary_int_op!(neg, Opcode::Neg);
    unary_int_op!(inv, Opcode::Inv);
    unary_int_op_with_overflow!(incr, Opcode::Incr);
    unary_int_op!(ilog2, Opcode::Ilog2);
    unary_int_op!(pow2, Opcode::Pow2);
    unary_boolean_op!(not, Opcode::Not);
    unary_int_op!(bnot, Opcode::Bnot);
    unary_int_op!(popcnt, Opcode::Popcnt);
    unary_int_op!(clz, Opcode::Clz);
    unary_int_op!(ctz, Opcode::Ctz);
    unary_int_op!(clo, Opcode::Clo);
    unary_int_op!(cto, Opcode::Cto);
     */

    fn rotl(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Rotl, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn rotr(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Rotr, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn shl(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Shl, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn shr(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Shr, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn eq(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Eq, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn neq(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Neq, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn gt(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Gt, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn gte(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Gte, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn lt(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Lt, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn lte(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Lte, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    #[allow(clippy::wrong_self_convention)]
    fn is_odd(mut self, value: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::IsOdd, _>(span);
        let op = op_builder(value)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn exec<C, A>(
        mut self,
        callee: C,
        args: A,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::Exec>, Report>
    where
        C: AsCallableSymbolRef,
        A: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Exec, (C, A)>(span);
        op_builder(callee, args)
    }

    /*
    fn call(mut self, callee: FunctionIdent, args: &[Value], span: SourceSpan) -> Inst {
        let mut vlist = ValueList::default();
        {
            let dfg = self.data_flow_graph_mut();
            assert!(
                dfg.get_import(&callee).is_some(),
                "must import callee ({}) before calling it",
                &callee
            );
            vlist.extend(args.iter().copied(), &mut dfg.value_lists);
        }
        self.Call(Opcode::Call, callee, vlist, span).0
    }
     */

    fn select(
        mut self,
        cond: ValueRef,
        a: ValueRef,
        b: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Select, _>(span);
        let op = op_builder(cond, a, b)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn br<A>(
        mut self,
        block: BlockRef,
        args: A,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::Br>, Report>
    where
        A: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Br, (_, A)>(span);
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
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::CondBr>, Report>
    where
        T: IntoIterator<Item = ValueRef>,
        F: IntoIterator<Item = ValueRef>,
    {
        let op_builder =
            self.builder_mut().create::<crate::dialects::hir::CondBr, (_, _, T, _, F)>(span);
        op_builder(cond, then_dest, then_args, else_dest, else_args)
    }

    /*
    fn switch(self, arg: ValueRef, span: SourceSpan) -> SwitchBuilder<'f, Self> {
        require_integer!(self, arg, Type::U32);
        SwitchBuilder::new(self, arg, span)
    }
     */

    fn ret(
        mut self,
        returning: Option<ValueRef>,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::Ret>, Report> {
        let op_builder = self
            .builder_mut()
            .create::<crate::dialects::hir::Ret, (<Option<ValueRef> as IntoIterator>::IntoIter,)>(
                span,
            );
        op_builder(returning)
    }

    fn ret_imm(
        mut self,
        arg: Immediate,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::RetImm>, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::RetImm, _>(span);
        op_builder(arg)
    }

    fn unreachable(
        mut self,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::hir::Unreachable>, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::hir::Unreachable, _>(span);
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

impl<'f, T: InstBuilderBase<'f>> InstBuilder<'f> for T {}

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
