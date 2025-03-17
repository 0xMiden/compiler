use midenc_hir2::{
    dialects::builtin::*, AsCallableSymbolRef, Builder, Immediate, Op, OpBuilder, Report,
    Signature, SourceSpan, Type, UnsafeIntrusiveEntityRef, ValueRef,
};

use crate::*;

pub trait HirOpBuilder<'f, B: ?Sized + Builder> {
    fn assert(
        &mut self,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Assert>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Assert, (ValueRef,)>(span);
        op_builder(value)
    }

    fn assert_with_error(
        &mut self,
        value: ValueRef,
        code: u32,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Assert>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Assert, (ValueRef, u32)>(span);
        op_builder(value, code)
    }

    fn assertz(
        &mut self,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Assertz>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Assertz, (ValueRef,)>(span);
        op_builder(value)
    }

    fn assertz_with_error(
        &mut self,
        value: ValueRef,
        code: u32,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Assertz>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Assertz, (ValueRef, u32)>(span);
        op_builder(value, code)
    }

    fn assert_eq(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::AssertEq>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::AssertEq, _>(span);
        op_builder(lhs, rhs)
    }

    fn assert_eq_imm(
        &mut self,
        lhs: ValueRef,
        rhs: Immediate,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::AssertEq>, Report> {
        use midenc_dialect_arith::ArithOpBuilder;
        let rhs = self.builder_mut().imm(rhs, span);
        self.assert_eq(lhs, rhs, span)
    }

    /// Grow the global heap by `num_pages` pages, in 64kb units.
    ///
    /// Returns the previous size (in pages) of the heap, or -1 if the heap could not be grown.
    fn mem_grow(&mut self, num_pages: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::MemGrow, _>(span);
        let op = op_builder(num_pages)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Return the size of the global heap in pages, where each page is 64kb.
    fn mem_size(&mut self, span: SourceSpan) -> Result<ValueRef, Report> {
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
        &mut self,
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
    fn symbol_addr<S: AsRef<str>>(self, name: S, ty: Type, span: SourceSpan) -> ValueRef {
        todo!()
        // self.symbol_relative_addr(name, 0, ty, span)
    }

    /// Same semantics as `symbol_addr`, but applies a constant offset to the address of the given
    /// symbol.
    ///
    /// If the offset is zero, this is equivalent to `symbol_addr`
    fn symbol_relative_addr<S: AsRef<str>>(
        &mut self,
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
    fn load_symbol<S: AsRef<str>>(
        &mut self,
        name: S,
        ty: Type,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        self.load_symbol_relative(name, ty, 0, span)
    }

    /// Same semantics as `load_symbol`, but a constant offset is applied to the address before
    /// issuing the load.
    fn load_symbol_relative<S: AsRef<str>>(
        &mut self,
        name: S,
        ty: Type,
        offset: i32,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let base = self.data_flow_graph_mut().create_global_value(GlobalValueData::Symbol {
            name: Ident::new(Symbol::intern(name.as_ref()), span),
            offset: 0,
        });
        self.load_global_relative(base, ty, offset, span)
    }

    */

    /// Loads a value of type `ty` from the address represented by `addr`
    ///
    /// NOTE: There is no requirement that the memory contents at the given symbol
    /// contain a valid value of type `ty`. That is left entirely up the caller to
    /// guarantee at a higher level.
    fn load_global(
        &mut self,
        addr: GlobalVariableRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        self.load_global_relative(addr, 0, span)
    }

    /// Loads a value from a global variable.
    ///
    /// A constant offset is applied to the address before issuing the load.
    fn load_global_relative(
        &mut self,
        base: GlobalVariableRef,
        offset: i32,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        // let base = &base.borrow();
        let gs_builder = GlobalSymbolBuilder::new(self.builder_mut(), span);
        let global_sym = gs_builder(base, offset)?;
        let addr = global_sym.borrow().results()[0].borrow().as_value_ref();
        let ty = base.borrow().ty().clone();
        let typed_addr = self.bitcast(addr, Type::Ptr(ty.into()), span)?;
        self.load(typed_addr, span)
    }

    /// Stores `value` to the global variable
    fn store_global(
        &mut self,
        global_var: GlobalVariableRef,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Store>, Report> {
        let gs_builder = GlobalSymbolBuilder::new(self.builder_mut(), span);
        let global_sym = gs_builder(global_var, 0)?;
        let addr = global_sym.borrow().results()[0].borrow().as_value_ref();
        let ty = global_var.borrow().ty().clone();
        let typed_addr = self.bitcast(addr, Type::Ptr(ty.into()), span)?;
        self.store(typed_addr, value, span)
    }

    /*

    /// Computes an address relative to the pointer produced by `base`, by applying an offset
    /// given by multiplying `offset` * the size in bytes of `unit_ty`.
    ///
    /// The type of the pointer produced is the same as the type of the pointer given by `base`
    ///
    /// This is useful in some scenarios where `load_global_relative` is not, namely when computing
    /// the effective address of an element of an array stored in a global variable.
    fn global_addr_offset(
        &mut self,
        base: GlobalValue,
        offset: i32,
        unit_ty: Type,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
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
    fn load(&mut self, addr: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Load, _>(span);
        let op = op_builder(addr)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Loads a value of the type of the given local variable, on to the stack
    ///
    /// NOTE: This function will panic if `local` is not valid within the current function
    fn load_local(&mut self, local: LocalVariable, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::LoadLocal, _>(span);
        let op = op_builder(local)?;
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
        &mut self,
        ptr: ValueRef,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Store>, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Store, _>(span);
        op_builder(ptr, value)
    }

    /// Stores `value` to the given local variable.
    ///
    /// NOTE: This function will panic if the local variable and value types do not match
    fn store_local(
        &mut self,
        local: LocalVariable,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::StoreLocal>, Report> {
        assert_eq!(
            value.borrow().ty(),
            &local.ty(),
            "cannot store a value of a different type in the given local variable"
        );
        let op_builder = self.builder_mut().create::<crate::ops::StoreLocal, _>(span);
        op_builder(local, value)
    }

    /*

    /// Stores `value` to the given temporary (local variable).
    ///
    /// NOTE: This function will panic if the type of `value` does not match the type of the local
    /// variable.
    fn store_local(&mut self, local: LocalId, value: Value, span: SourceSpan) -> Inst {
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
        &mut self,
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
        &mut self,
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
    fn ptrtoint(&mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
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
    fn inttoptr(&mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
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
    fn getelementptr(&mut self, ptr: ValueRef, mut indices: &[usize], span: SourceSpan) -> Result<ValueRef, Report> {
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
    /// NOTE: This is only valid for numeric to numeric.
    /// For numeric to pointer, or pointer to numeric casts, use `inttoptr` and `ptrtoint`
    /// respectively.
    fn cast(&mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Cast, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn exec<C, A>(
        &mut self,
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
        &mut self,
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

    fn builder(&self) -> &B;
    fn builder_mut(&mut self) -> &mut B;
}

impl<'f, B: ?Sized + Builder> HirOpBuilder<'f, B> for FunctionBuilder<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        FunctionBuilder::builder(self)
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        FunctionBuilder::builder_mut(self)
    }
}

impl<'f> HirOpBuilder<'f, OpBuilder> for &'f mut OpBuilder {
    #[inline(always)]
    fn builder(&self) -> &OpBuilder {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut OpBuilder {
        self
    }
}

impl<B: ?Sized + Builder> HirOpBuilder<'_, B> for B {
    #[inline(always)]
    fn builder(&self) -> &B {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self
    }
}
