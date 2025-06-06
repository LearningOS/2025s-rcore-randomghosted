本次实验我主要实现了对sys_mmap、sys_unmap、sys_spawn等进程相关的系统调用的实现，并且按定义实现了stride调度算法



lab3.md:

1）不是，因为250+10=260，在8bit表示下会溢出，进而还是会比p1.stride小，实际情况会轮到p2执行

2）因为一开始所有进程的stride都相等，在所有进程优先级>=2的时候stride的增值不超过BigStride/2，而Stride算法每次都会选择Stride最小的去调度，所以在所有进程优先级>=2的情况下，Stride_max-Stride_min<=BigStride/2

3）

use core::cmp::Ordering;

use config:BIG_STRIDE;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let max=self.0.max(other.0);

        let min=self.0.min(other.0);

        if max-min>BIG_STRIDE/2{

            if self.0==max{ Some(Ordering::Less)}

            else {Some(Ordering::Greater)}

        }else{

            if self.0==max{Some(Ordering::Greater)}

            else{Some(Ordering::Less)}

        }

    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false
    }
}



1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：
   
   > 无

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：
   
   > 无

3. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
