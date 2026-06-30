pub(crate) fn index_from_top<T>(stack: &[T], depth: usize) -> Option<usize> {
    if stack.len() <= depth {
        None
    } else {
        Some(stack.len() - 1 - depth)
    }
}

pub(crate) fn pop_chunk<T>(stack: &mut Vec<T>, chunk_len: usize) -> Option<Vec<T>> {
    if stack.len() < chunk_len {
        return None;
    }
    Some(stack.split_off(stack.len() - chunk_len))
}

pub(crate) fn dup<T: Clone>(stack: &mut Vec<T>, depth: usize) -> Option<()> {
    let index = index_from_top(stack, depth)?;
    stack.push(stack[index].clone());
    Some(())
}

pub(crate) fn dup_word<T: Clone>(stack: &mut Vec<T>, depth: usize) -> Option<()> {
    for _ in 0..4 {
        dup(stack, depth.checked_mul(4)?.checked_add(3)?)?;
    }
    Some(())
}

pub(crate) fn swap<T>(stack: &mut [T], depth: usize) -> Option<()> {
    let index = index_from_top(stack, depth)?;
    let top = index_from_top(stack, 0)?;
    stack.swap(index, top);
    Some(())
}

pub(crate) fn swap_chunks<T>(stack: &mut [T], chunk_len: usize, depth: usize) -> Option<()> {
    let total = chunk_len.checked_mul(depth.checked_add(1)?)?;
    if stack.len() < total {
        return None;
    }
    let len = stack.len();
    let top_start = len - chunk_len;
    let other_start = len - total;
    for offset in 0..chunk_len {
        stack.swap(other_start + offset, top_start + offset);
    }
    Some(())
}

pub(crate) fn movup<T>(stack: &mut Vec<T>, depth: usize) -> Option<()> {
    let index = index_from_top(stack, depth)?;
    let value = stack.remove(index);
    stack.push(value);
    Some(())
}

pub(crate) fn move_chunk_to_top<T>(
    stack: &mut Vec<T>,
    chunk_len: usize,
    depth: usize,
) -> Option<()> {
    let total = chunk_len.checked_mul(depth.checked_add(1)?)?;
    if stack.len() < total {
        return None;
    }
    let start = stack.len() - total;
    let chunk = stack.drain(start..start + chunk_len).collect::<Vec<_>>();
    stack.extend(chunk);
    Some(())
}

pub(crate) fn movdn<T>(stack: &mut Vec<T>, depth: usize) -> Option<()> {
    index_from_top(stack, depth)?;
    let value = stack.pop()?;
    let index = stack.len().saturating_sub(depth);
    stack.insert(index, value);
    Some(())
}

pub(crate) fn move_top_chunk_down<T>(
    stack: &mut Vec<T>,
    chunk_len: usize,
    depth: usize,
) -> Option<()> {
    let total = chunk_len.checked_mul(depth.checked_add(1)?)?;
    if stack.len() < total {
        return None;
    }
    let len = stack.len();
    let chunk = stack.drain(len - chunk_len..).collect::<Vec<_>>();
    let index = stack.len().checked_sub(chunk_len.checked_mul(depth)?)?;
    stack.splice(index..index, chunk);
    Some(())
}

pub(crate) fn reverse_n<T>(stack: &mut [T], n: usize) -> Option<()> {
    if stack.len() < n {
        return None;
    }
    let len = stack.len();
    stack[len - n..].reverse();
    Some(())
}
