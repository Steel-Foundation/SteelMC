// Run with Minecraft 26.2's server/library classpath, OpenJDK 25 and
// --add-opens java.base/java.util=ALL-UNNAMED. Collection oracle, not gameplay.
// The --sequential-identities mode also requires
// -XX:+UnlockExperimentalVMOptions -XX:hashCode=3.
import java.lang.reflect.*;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.*;
import net.minecraft.core.BlockPos;

public class HashSetOracle {
    static Field map, table, key, left, right, red;
    static boolean sequentialIdentities;
    static int previousIdentity;
    static HashSet<BlockPos> set;
    static void start(String name) {System.out.println("case " + name);set=new HashSet<>();}
    static void add(int spread) throws Exception {
        // Invert HashMap.hash to choose a signed spread hash, using real BlockPos.
        add(new BlockPos(spread^(spread>>>16), 0, 0));
    }
    static void add(BlockPos pos) throws Exception {
        if(sequentialIdentities) {
            int identity=System.identityHashCode(pos);
            if(identity<=previousIdentity)throw new IllegalStateException("sequential identity hashes required: use -XX:hashCode=3");
            previousIdentity=identity;
        }
        boolean added=set.add(pos);
        Object[] bins=(Object[])table.get(map.get(set));
        long trees=Arrays.stream(bins).filter(b->b!=null&&b.getClass().getSimpleName().equals("TreeNode")).count();
        StringBuilder order=new StringBuilder();
        for(BlockPos p:set) order.append(p.getX()).append(' ').append(p.getY()).append(' ').append(p.getZ()).append('\n');
        String digest=HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(order.toString().getBytes(StandardCharsets.UTF_8)));
        StringBuilder shape=new StringBuilder();
        for(int i=0;i<bins.length;i++)if(bins[i]!=null&&bins[i].getClass().getSimpleName().equals("TreeNode")) {shape.append(i).append(':');shape(bins[i],shape);}
        String shapeDigest=HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(shape.toString().getBytes(StandardCharsets.UTF_8)));
        System.out.printf("%d %d %d %s %d %d %s %s%n",pos.getX(),pos.getY(),pos.getZ(),added,bins.length,trees,digest,shapeDigest);
    }
    static void shape(Object node, StringBuilder out) throws Exception {
        if(node==null) {out.append('.');return;}
        BlockPos p=(BlockPos)key.get(node);
        out.append('(').append(p.getX()).append(',').append(p.getY()).append(',').append(p.getZ()).append(',').append(red.getBoolean(node)?'r':'b');
        shape(left.get(node),out);shape(right.get(node),out);out.append(')');
    }
    static void warmup() throws Exception {for(int h=1;h<=25;h++)add(h);}
    static void fillUntil(int size) throws Exception {for(int h=33;set.size()<size;h++)if((h&63)!=0)add(h);}
    public static void main(String[] args) throws Exception {
        sequentialIdentities=args.length==1&&args[0].equals("--sequential-identities");
        if(args.length>0&&!sequentialIdentities)throw new IllegalArgumentException("expected --sequential-identities");
        map=HashSet.class.getDeclaredField("map");map.setAccessible(true);
        key=Class.forName("java.util.HashMap$Node").getDeclaredField("key");key.setAccessible(true);
        Class<?> tree=Class.forName("java.util.HashMap$TreeNode");
        left=tree.getDeclaredField("left");left.setAccessible(true);right=tree.getDeclaredField("right");right.setAccessible(true);red=tree.getDeclaredField("red");red.setAccessible(true);
        table=HashMap.class.getDeclaredField("table");table.setAccessible(true);
        if(sequentialIdentities) {
            start("equal-hash-sequential-identities");
            for(int z=0;z<40;z++)add(new BlockPos(-961*z,0,z));
            for(int z=0;z<40;z++)add(new BlockPos(-961*z,0,z));
            start("equal-hash-rebuild-after-split");warmup();
            for(int z=0;z<12;z++)add(new BlockPos(-961*z,0,z));
            for(int i=0;i<8;i++)add(64+i*128);
            fillUntil(49);add(new BlockPos(-961*12,0,12));
            fillUntil(97);add(new BlockPos(-961*13,0,13));
            return;
        }
        start("control-initial-sixteen");for(int h=0;h<12;h++)add(h);add(3);
        start("control-resize-without-trees");for(int h=0;h<130;h++)add(h);
        start("collision-resizes-before-treeification");for(int h=0;h<15;h++)add(h*64);add(128);
        start("split-to-lists-and-treeify-again");warmup();for(int h=0;h<9;h++)add(h*64);fillUntil(49);for(int h=9;h<21;h++)add(h*64);add(640);
        start("split-tree-and-list");warmup();for(int h=0;h<13;h++)add(h*128);for(int h=0;h<3;h++)add(h*128+64);fillUntil(49);add(13*128);add(64);
        start("split-two-trees-then-untreeify");warmup();for(int h=0;h<16;h++)add(h*64);fillUntil(49);add(1024);add(1088);fillUntil(97);add(1152);
        start("unsplit-tree-retains-shape");warmup();for(int h=0;h<14;h++)add(h*256);fillUntil(49);add(14*256);fillUntil(97);add(15*256);fillUntil(193);add(16*256);
        start("overflowing-three-dimensional-hashes");warmup();
        Random coords=new Random(428);
        for(int i=0;i<25;i++) {
            int spread=(i%2==0?Integer.MIN_VALUE:0)+(i/2)*64;
            int raw=spread^(spread>>>16), y=coords.nextInt(), z=coords.nextInt();
            add(new BlockPos(raw-(y+z*31)*31,y,z));
        }
        start("signed-hash-comparison");warmup();for(int h:new int[]{0, Integer.MIN_VALUE, 64, Integer.MIN_VALUE+64, -64, 128, -128, Integer.MIN_VALUE+128, 192, -192, Integer.MAX_VALUE-63})add(h);add(Integer.MIN_VALUE);

    }
}
